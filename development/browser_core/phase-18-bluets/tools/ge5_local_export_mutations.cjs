// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every coherent attack re-enters the full original module, with fresh Native coverage.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{coherent}=require('./ge5_decorator_preserved_mutations.cjs');
function run(ts,native,actual,options,original,proof){const controls=[],source=ts.createSourceFile('original.ts',original,ts.ScriptTarget.Latest,true),claimed=new Set(native.decoded.segments.filter(point=>point.length>2).map(point=>JSON.stringify(point.slice(2)))),column=Array.from({length:original.split('\n')[0].length+1},(_,index)=>index).find(column=>!claimed.has(JSON.stringify([0,0,column])));assert.notEqual(column,undefined);const wrong=[0,0,column];
 const complete=view=>require('./ge5_metadata_class_outputs.cjs').compare(ts,stream(ts,native.code,native.raw),view,options,original);complete(stream(ts,actual.code,actual.raw));
 function rejected(label,view,offset){assert.throws(()=>complete(view),undefined,label);controls.push({label,offset,rejected:true});}
 function mapping(label,offset,owner){assert.notDeepEqual(actual.owner(offset),owner);rejected(label,coherent(ts,mutate(ts,actual,offset,owner)),offset);}
 function code(label,start,end,replacement){assert.equal(replacement.length,end-start);assert.notEqual(replacement,actual.code.slice(start,end));const view=stream(ts,actual.code.slice(0,start)+replacement+actual.code.slice(end),actual.raw);assert.equal(view.source.parseDiagnostics.length,0);rejected(label,coherent(ts,view),start);}
 const ends=new Set();function endpoint(label,offset){if(ends.has(offset))return;ends.add(offset);mapping(label+' retarget endpoint',offset,wrong);if(actual.owner(offset))mapping(label+' erase endpoint',offset,null);}
 for(const item of proof.classes)for(const pair of item.declarationChain)endpoint(pair.label,pair.actual.end);
 for(const pair of[...(proof.delegated.localExportPairs??[]),...(proof.delegated.reexportPairs??[])])endpoint(pair.label,pair.actual.end);
 const tokens=actual.tokens(actual.source);for(let i=0;i<tokens.length;i++){const token=tokens[i],mapped=token.owners.findIndex(Boolean);if(mapped>=0){const offset=token.start+Math.min(mapped+1,token.end-token.start-1);mapping('complete retained token UTF-16 retarget',offset,wrong);mapping('complete retained token UTF-16 erase',offset,null);}else if(i===0||tokens[i-1].owners.some(Boolean))mapping('generated syntax cannot acquire source',token.start,wrong);
 if(i+1<tokens.length){for(let offset=token.end;offset<tokens[i+1].start;offset++)if(actual.owner(offset)){mapping('complete ordered mapped gap retarget',offset,wrong);break;}}}
 const names=new Set();function visit(node){if(ts.isIdentifier(node)&&actual.owner(node.getStart(actual.source))&&!names.has(node.text)){names.add(node.text);const start=node.getStart(actual.source);code('exact original/forwarding binding '+node.text,start,start+1,node.text[0]==='Z'?'Y':'Z');}
 if(ts.isNumericLiteral(node)&&actual.owner(node.getStart(actual.source))&&node.text==='17')code('original provider initializer value',node.getStart(actual.source),node.end,'18');
 if(ts.isStringLiteral(node)&&(['Alias','x-y','default','Renamed','Result'].includes(node.text)||node.text==='./types.js'||node.text==='./barrel.js')){const start=node.getStart(actual.source)+1;code('exact original exported key or module edge',start,start+1,'Z');}
 if(ts.isFunctionExpression(node)&&!node.name&&!node.asteriskToken){const keyword=actual.tokens(node).find(token=>token.text==='function');if(keyword&&actual.code[keyword.start-1]===' '){code('ordinary forwarding factory/callable kind',keyword.start-1,keyword.end,'function*');}}
 ts.forEachChild(node,visit);}
 visit(actual.source);
 for(const comment of actual.comments()){const label=comment.text.startsWith('//# sourceMappingURL=')?'publication':'comment';for(let offset=comment.start;offset<comment.end;offset++)if(actual.owner(offset)){mapping(label+' UTF-16 source retarget',offset,wrong);mapping(label+' UTF-16 source erase',offset,null);}endpoint(label+' logical end',comment.end);}
 for(const[field,value]of[['sourceRoot','../forged'],['names',['forged']],['file','forged.js']]){const view=stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),[field]:value}));assert.throws(()=>{assert.deepEqual(view.decoded.metadata,native.decoded.metadata);complete(view);});controls.push({label:'complete map metadata '+field,rejected:true});}
 return controls;
}
module.exports={run};
