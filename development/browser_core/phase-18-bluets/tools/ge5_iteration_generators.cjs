// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Import identity is established independently before reading either generator ABI.
const assert=require('node:assert/strict'),envelope=require('./ge5_module_envelope.cjs'),machine=require('./ge5_continuation_machine.cjs'),linear=require('./ge5_linear_roles.cjs'),{compareTokens}=require('./ge5_role_stream.cjs');
function compare(ts,native,actual,options,original) {
  assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);
  const es5=options.target==='ES5',external=es5&&(options.importHelpers||options.noEmitHelpers),dependencies=es5&&options.importHelpers?['tslib']:[];
  function read(view,blue) {
    const item=envelope.read(ts,view,options,{blue,originals:[original],dependencies});
    const functions=item.sourceNodes.filter(node=>ts.isFunctionDeclaration(node)&&node.name?.text==='values');assert.equal(functions.length,1,'one complete original values function');const fn=functions[0];
    let reference=null;
    if(external) {
      if(!options.importHelpers)reference={binding:'__generator',member:'__generator',abi:blue?'tslib-adapter':'native'};
      else {
        let namespace=item.parameterBindings[0]?.binding||null,binding=null;
        if(item.format==='system') {
          assert.equal(item.setterNodes.length,1);const setter=item.setterNodes[0],parameter=setter.parameters[0].name.text,assignment=setter.body.statements[blue?1:0];assert(ts.isExpressionStatement(assignment)&&ts.isBinaryExpression(assignment.expression)&&ts.isIdentifier(assignment.expression.left));namespace=assignment.expression.left.text;
          item.exact(setter,blue?`function(${parameter}){${item.system.stem}_imports['tslib']=${parameter};${namespace}=${parameter};${item.system.stem}_refresh();}`:`function(${parameter}){${namespace}=${parameter};}`);view.noSource(setter.getStart(view.source),setter.end,'generated iteration helper setter');
        }
        const imports=[];
        for(const node of item.sourceNodes) {
          if(ts.isImportDeclaration(node)){assert(ts.isStringLiteral(node.moduleSpecifier)&&node.moduleSpecifier.text==='tslib');if(blue){assert(ts.isNamespaceImport(node.importClause.namedBindings));namespace=node.importClause.namedBindings.name.text;item.exact(node,`import * as ${namespace} from 'tslib';`);}else{assert(ts.isNamedImports(node.importClause.namedBindings));const members=node.importClause.namedBindings.elements.map(e=>e.propertyName?.text||e.name.text);assert.deepEqual(members,['__generator','__read','__values']);binding=node.importClause.namedBindings.elements.find(e=>(e.propertyName?.text||e.name.text)==='__generator').name.text;}imports.push(node);}
          else if(ts.isVariableStatement(node)&&node.declarationList.declarations.length===1){const d=node.declarationList.declarations[0],init=d.initializer;if(init&&ts.isCallExpression(init)&&init.expression.getText()==='require'){assert(ts.isIdentifier(d.name)&&init.arguments.length===1&&ts.isStringLiteral(init.arguments[0])&&init.arguments[0].text==='tslib');namespace=d.name.text;item.exact(node,`${blue||options.target==='ES5'?'var':'const'} ${namespace}=require('tslib');`);imports.push(node);}}
        }
        assert.equal(imports.length,item.format==='system'||!blue&&item.parameterBindings.length?0:1,'one complete helper import');for(const node of imports)view.noSource(node.getStart(view.source),node.end,'generated iteration helper import');
        assert(Boolean(namespace)!==Boolean(binding));reference={...(namespace?{namespace}:{binding}),member:'__generator',abi:blue?'tslib-adapter':'native'};
      }
    }
    item.classifyTransport();return {...item,fn,reference,blue};
  }
  const n=read(native,false),a=read(actual,true);
  let proof=null;
  if(es5){const nm=machine.read(ts,native,n.fn,'native',[original],{exceptions:'normal-finally',...(n.reference?{generatorReference:n.reference}:{})}),am=machine.read(ts,actual,a.fn,'blue',[original],{exceptions:'normal-finally',...(a.reference?{generatorReference:a.reference}:{})});proof=linear.compareFunction(ts,native,nm,actual,am);proof.machines={native:nm,actual:am};}
  else compareTokens(native,native.tokens(n.fn),actual,actual.tokens(a.fn),'complete retained iterator generator');
  for(const [view,item]of [[native,n],[actual,a]])for(let offset=item.fn.getStart(view.source);offset<item.fn.end;offset++)assert(view.covered(offset),'complete generator UTF-16 role inventory at '+offset);
  return {native:n,actual:a,continuation:proof};
}
module.exports={compare};
