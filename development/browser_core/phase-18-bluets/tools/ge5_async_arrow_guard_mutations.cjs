// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Isolated, publication-coherent controls challenge the closed guard inventory.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs');
function coherent(ts,view){if(view.code.includes('//# sourceMappingURL=data:')){const encoded=Buffer.from(view.raw).toString('base64'),code=view.code.replace(/(\/\/# sourceMappingURL=data:application\/json;base64,)[^\r\n]+/,'$1'+encoded);return stream(ts,code,view.raw);}return view;}
function run(ts,view,proof,check){const controls=[],start=node=>node.getStart(view.source);function code(label,node,text){const from=start(node);assert(text.length<=node.end-from);const changed=coherent(ts,stream(ts,view.code.slice(0,from)+text.padEnd(node.end-from,' ')+view.code.slice(node.end),view.raw));assert.throws(()=>check(changed),undefined,label);controls.push({label,rejected:true,publicationCoherent:true});}
  function mapping(label,offset,owner){const changed=coherent(ts,mutate(ts,view,offset,owner));assert.throws(()=>check(changed),undefined,label);controls.push({label,rejected:true,publicationCoherent:true});}
  code('invert default predicate',proof.condition.operatorToken,'!==');
  const dispatch=proof.actualMachine.dispatcher.caseBlock.clauses.find(clause=>clause.expression?.text===String(proof.branch)),selection=dispatch.statements[0].expression.right;
  code('read a different predicate slot',selection.condition,proof.slot.slice(0,-1)+(proof.slot.at(-1)==='9'?'8':'9'));
  code('skip selected default state',selection.whenTrue,String(proof.core));code('default on the provided path',selection.whenFalse,String(proof.defaultState));
  code('change selected default value',proof.defaultValue,'0');code('erase selected default assignment',proof.defaultAssignment,'');
  code('skip guard entry',proof.actualMachine.call.arguments[1],String(proof.core));code('erase dispatch state',dispatch,'');
  mapping('erase default predicate UTF-16 interior',start(proof.condition.left)+1,null);mapping('erase captured predicate prefix',start(proof.predicate)+1,null);
  mapping('erase default assignment ending',proof.defaultAssignment.end-1,null);mapping('grant routing state source',start(dispatch)+1,[0,0,0]);
  const declaration=proof.actualMachine.node.body.statements.flatMap(node=>ts.isVariableStatement(node)?[...node.declarationList.declarations]:[]).find(node=>node.name.getText()===proof.slot);assert(declaration);mapping('grant predicate capture declaration source',start(declaration)+1,[0,0,0]);
  return controls;
}
module.exports={run,coherent};
