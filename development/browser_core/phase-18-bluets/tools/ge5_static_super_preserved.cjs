// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Preserved source classes prove every module character on fresh independent views.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate,encode}=require('./ge5_role_mutations.cjs'),preserved=require('./ge5_decorator_preserved.cjs'),{coherent}=require('./ge5_decorator_preserved_mutations.cjs');
function run(ts,native,actual,original){
 const freshN=()=>stream(ts,native.code,native.raw),freshA=()=>stream(ts,actual.code,actual.raw);
 function check(view){const n=freshN(),a=coherent(ts,view),proof=preserved.compare(ts,n,a);function ends(nn,an){assert.equal(an.kind,nn.kind,'complete preserved module AST kind');assert.deepEqual(a.owner(an.end),n.owner(nn.end),'exact preserved module AST endpoint '+ts.SyntaxKind[nn.kind]);const nc=[],ac=[];ts.forEachChild(nn,c=>{nc.push(c);});ts.forEachChild(an,c=>{ac.push(c);});assert.equal(ac.length,nc.length,'complete preserved module AST child inventory');for(let i=0;i<nc.length;i++)ends(nc[i],ac[i]);}ends(n.source,a.source);return proof;}
 const proof=check(freshA()),controls=[],seen=new Set();
 const claimed=new Set(native.decoded.segments.filter(p=>p.length>2).map(p=>JSON.stringify(p.slice(2))));
 const column=Array.from({length:original.split('\n')[0].length+1},(_,column)=>column).find(column=>!claimed.has(JSON.stringify([0,0,column])));assert.notEqual(column,undefined);const wrong=[0,0,column];
 function mapping(label,offset,owner){const key=offset+':'+JSON.stringify(owner);if(seen.has(key))return;seen.add(key);assert.notDeepEqual(actual.owner(offset),owner,'effective preserved source-role mutation');const changed=coherent(ts,mutate(ts,freshA(),offset,owner));assert.throws(()=>check(changed),undefined,label);controls.push({label,offset,rejected:true,kind:'fresh-coherent-map'});}
 for(const[index,token]of proof.actualTokens.entries()){
  const offset=token.start+Number(token.end-token.start>1);mapping('complete preserved token '+index,offset,wrong);if(actual.owner(offset)!==null)mapping('erase preserved token '+index,offset,null);
  if(index)for(let at=proof.actualTokens[index-1].end;at<token.start;at++)if(actual.owner(at)!==null){mapping('ordered preserved gap '+index,at,wrong);break;}
 }
 function ends(node){if(node.kind!==ts.SyntaxKind.EndOfFileToken&&node.end<actual.code.length){mapping('exact preserved '+ts.SyntaxKind[node.kind]+'.end',node.end,wrong);if(actual.owner(node.end)!==null)mapping('erase exact preserved '+ts.SyntaxKind[node.kind]+'.end',node.end,null);}ts.forEachChild(node,ends);}ends(actual.source);
 const publication=proof.actualComments.generated[0];mapping('preserved map publication has no source authority',publication.start+4,wrong);
 for(const comment of proof.actualComments.source)for(let at=comment.start;at<=comment.end;at++){mapping('preserved source comment UTF-16',at,wrong);if(actual.owner(at)!==null)mapping('erase preserved source comment UTF-16',at,null);}
 const point=actual.source.getLineAndCharacterOfPosition(publication.start+4),segments=actual.decoded.segments.map(p=>[...p]),where=segments.findIndex(p=>p[0]>point.line||p[0]===point.line&&p[1]>=point.character);segments.splice(where<0?segments.length:where,0,[point.line,point.character,...wrong],[point.line,point.character]);assert.throws(()=>check(coherent(ts,stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),mappings:encode(segments)})))),undefined,'hidden preserved zero-width source authority');controls.push({label:'hidden preserved zero-width source authority',rejected:true});
 for(const[field,value]of[['sourceRoot','../forged/'],['names',['forged']]]){assert.throws(()=>check(coherent(ts,stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),[field]:value})))),undefined,'preserved complete metadata '+field);controls.push({label:'preserved complete metadata '+field,rejected:true});}
 const number=proof.actualTokens.find(t=>/^\d$/.test(t.text));assert(number);const code=actual.code.slice(0,number.start)+(number.text==='0'?'1':'0')+actual.code.slice(number.end);assert.notEqual(code,actual.code);assert.throws(()=>check(stream(ts,code,actual.raw)),undefined,'changed preserved original operand');controls.push({label:'changed preserved original operand',rejected:true,kind:'complete-source-AST'});
 assert.throws(()=>check(stream(ts,actual.code+'\n/* forged generated comment */\n',actual.raw)),undefined,'complete preserved comment inventory');controls.push({label:'complete preserved comment inventory',rejected:true});
 return {completeModuleRoles:true,controls};
}
module.exports={run};
