// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),{comments}=require('./ge5_decorator_preserved.cjs');
function verify(ts,native,actual,options,originalText){
 const original=ts.createSourceFile('original.ts',originalText,99,true),originalComments=comments(ts,{code:originalText}).map(c=>c.text),asset=comments(ts,{code:fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/class_helpers.v1.js'),'utf8')}).map(c=>c.text);let count=0;function visit(node){if(ts.isClassDeclaration(node)||ts.isClassExpression(node))count++;ts.forEachChild(node,visit);}visit(original);assert.equal(count,2,'complete original replacement/source callable class inventory');
 for(const[view,blue]of[[native,false],[actual,true]]){const all=comments(ts,view),publications=all.filter(c=>c.text.startsWith('//# sourceMappingURL='));assert.equal(publications.length,1,'one complete source-map publication');const footer=publications[0];if(footer.text.includes('base64,'))assert.equal(Buffer.from(footer.text.split('base64,')[1],'base64').toString('utf8'),view.raw,'coherent complete inline publication');const expected=options.target==='ES5'?blue?Array.from({length:count},()=>asset).flat():Array(count).fill('/** @class */'):[];assert.deepEqual(all.filter(c=>c!==footer&&!originalComments.includes(c.text)).map(c=>c.text),expected,'complete AST-derived generated helper/comment inventory');}
 const footer=view=>comments(ts,view).find(c=>c.text.startsWith('//# sourceMappingURL='));if(!footer(native).text.includes('base64,'))assert.equal(footer(actual).text,footer(native).text,'exact external publication identity');
 const lines=originalText.split('\n');let previous;for(const point of actual.decoded.segments){assert(!previous||point[0]>previous[0]||point[0]===previous[0]&&point[1]>previous[1],'strictly ordered unique complete decoded source-map entries');previous=point;if(point.length>2)assert(point[2]===0&&point[3]<lines.length&&point[4]<=lines[point[3]].length,'complete source coordinates remain inside the original UTF-16 source');}
}
function ends(ts,native,actual){
 const nodes=view=>{const found=new Map();function visit(node){if(!ts.isToken(node)){if(!found.has(node.end))found.set(node.end,new Set());found.get(node.end).add(node.kind);}ts.forEachChild(node,visit);}visit(view.source);return found;},n=nodes(native),a=nodes(actual);let checked=0;
 for(const[observed,wanted]of actual.nativeEnds??[]){const nn=n.get(wanted),an=a.get(observed);if(!nn||!an||![...nn].some(kind=>an.has(kind)))continue;assert.deepEqual(actual.owner(observed),native.owner(wanted),'exact complete paired logical subtree endpoint at '+observed+' Native '+wanted+' '+JSON.stringify(actual.code.slice(Math.max(0,observed-24),observed+12))+' vs '+JSON.stringify(native.code.slice(Math.max(0,wanted-24),wanted+12)));checked++;}return checked;
}

module.exports={verify,ends};
