// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Each original callable composes only with complete class-value expression proofs.
const assert=require('node:assert/strict'),functions=require('./ge5_class_expression_functions.cjs'),plain=require('./ge5_class_expression_plain.cjs');
function compare(ts,native,actual,original,options,contract) {
  const proofs=[],classes=[],deferred=[];
  const emitted=(view,source)=>{const nodes=view.source.statements.filter(node=>ts.isFunctionDeclaration(node)&&node.name?.text===source.name.text);assert.equal(nodes.length,1,'one complete emitted original callable '+source.name.text);return nodes[0];};
  for(const source of original.statements.filter(ts.isFunctionDeclaration)) {
    const expressions=[];function visit(node){if(ts.isClassExpression(node)){expressions.push(node);return;}ts.forEachChild(node,visit);}visit(source);
    if(expressions.some(node=>(ts.getDecorators(node)||[]).length||node.members.some(member=>(ts.getDecorators(member)||[]).length))){deferred.push(source);continue;}
    const nf=emitted(native,source),af=emitted(actual,source),pairedExpressions=[],extraNativeNodes=[];
    for(const expression of expressions) {
      assert(ts.isReturnStatement(expression.parent)&&expression.parent.parent===source.body,'closed direct returned class-value callable');
      const returned=view=>{const nodes=(view===native?nf:af).body.statements.filter(ts.isReturnStatement);assert.equal(nodes.length,1,'one complete returned class-value source position');return nodes[0];};
      const proof=plain.compare(ts,native,returned(native),actual,returned(actual),expression,options,contract);pairedExpressions.push(proof.pairedExpression);extraNativeNodes.push(...proof.extraNativeNodes);classes.push(proof);
    }
    proofs.push(functions.compare(ts,native,nf,actual,af,source,options,{pairedExpressions,extraNativeNodes}));
  }
  return {proofs,classes,deferred,scope:'complete undecorated source callable trees; decorated enclosing callables are explicit pending AST nodes'};
}
module.exports={compare};
