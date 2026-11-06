// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Generated auto-storage uses the complete private helper algorithm without a source-private identity.
const assert=require('node:assert/strict'),semantic=require('./ge5_decorator_helpers.cjs').semantic;
const abi={native:new Map([['__classPrivateFieldGet','get'],['__classPrivateFieldSet','set']]),actual:new Map([['__bluetsClassPrivateGet','get'],['__bluetsClassPrivateSet','set']])};
function compare(ts,native,actual,original,options){
 assert(!options.importHelpers&&!options.noEmitHelpers,'the frozen auto-accessor source family uses complete inline helpers');
 const sourceNames=new Set();function names(node){if(ts.isIdentifier(node))sourceNames.add(node.text);ts.forEachChild(node,names);}names(original);
 function read(view,blue){const definitions=new Map(),bindings=new Map();function visit(node){if(ts.isVariableStatement(node))for(const declaration of node.declarationList.declarations){if(!ts.isIdentifier(declaration.name))continue;const role=abi[blue?'actual':'native'].get(declaration.name.text);if(!role)continue;assert.equal(node.declarationList.declarations.length,1);assert(!sourceNames.has(declaration.name.text)&&!definitions.has(role));let fn=declaration.initializer;if(!blue){assert(ts.isBinaryExpression(fn)&&fn.operatorToken.kind===ts.SyntaxKind.BarBarToken);let left=fn.left;while(ts.isParenthesizedExpression(left))left=left.expression;assert(ts.isBinaryExpression(left)&&left.operatorToken.kind===ts.SyntaxKind.AmpersandAmpersandToken&&left.left.kind===ts.SyntaxKind.ThisKeyword);assert(ts.isPropertyAccessExpression(left.right)&&left.right.expression.kind===ts.SyntaxKind.ThisKeyword&&left.right.name.text===declaration.name.text);fn=fn.right;}assert(ts.isFunctionExpression(fn)&&!fn.name&&!fn.asteriskToken&&!fn.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.AsyncKeyword));assert.equal(fn.parameters.length,role==='get'?4:5);definitions.set(role,{node,fn});bindings.set(declaration.name.text,role);}ts.forEachChild(node,visit);}visit(view.source);assert.deepEqual([...definitions.keys()].sort(),['get','set'],'complete auto-storage helper inventory');return{view,definitions,bindings};}
 const n=read(native,false),a=read(actual,true);
 for(const[role,definition]of n.definitions){const other=a.definitions.get(role);assert.deepEqual(semantic(ts,other.fn),semantic(ts,definition.fn),'complete auto-storage private '+role+' algorithm/flags/local bindings');for(const[state,item]of[[n,definition],[a,other]]){state.view.noSource(item.node.getStart(state.view.source),item.node.end,'complete generated auto-storage private helper');assert.equal(state.view.owner(item.fn.end),null,'private helper callable logical end stays generated');}}
 return{native:n,actual:a};
}
module.exports={compare};
