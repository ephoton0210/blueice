// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Each retained source callable owns its complete AST subtree and logical ends.
const assert=require('node:assert/strict');
function compare(ts,native,actual,source){
 function read(view){const found=[];function visit(node){if((ts.isClassDeclaration(node)||ts.isClassExpression(node))&&node.name?.text===source.name.text)found.push(node);ts.forEachChild(node,visit);}visit(view.source);assert.equal(found.length,1,'one exact retained source class identity');return found[0];}
 const n=read(native),a=read(actual),originals=source.members.filter(member=>ts.isMethodDeclaration(member)||ts.isGetAccessorDeclaration(member)||ts.isSetAccessorDeclaration(member)),points=[];
 function pair(nn,an){assert.equal(an.kind,nn.kind,'complete source callable subtree kind');assert.deepEqual(actual.owner(an.end),native.owner(nn.end),'exact original callable AST logical endpoint '+ts.SyntaxKind[nn.kind]);points.push({kind:ts.SyntaxKind[nn.kind],native:nn.end,actual:an.end,owner:native.owner(nn.end)});const nc=[],ac=[];ts.forEachChild(nn,child=>{nc.push(child);});ts.forEachChild(an,child=>{ac.push(child);});assert.equal(ac.length,nc.length,'complete source callable subtree child inventory');for(let index=0;index<nc.length;index++)pair(nc[index],ac[index]);}
 for(const original of originals){function member(item){const matches=item.members.filter(member=>member.kind===original.kind&&member.name?.text===original.name.text&&!!member.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.StaticKeyword)===!!original.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.StaticKeyword));assert.equal(matches.length,1,'one exact original callable kind/key/static identity');return matches[0];}pair(member(n),member(a));}
 return{source,endpoints:points.length,points};
}
module.exports={compare};
