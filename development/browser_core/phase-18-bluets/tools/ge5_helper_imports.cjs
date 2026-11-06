// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Exactly one reviewed tslib import binds the helper reference for a module.
const assert=require('node:assert/strict');
function create(ts,view,options,blue,identifiers,exact,generated,member) {
  const state={namespace:null,imported:false},external=options.importHelpers&&options.target==='ES5';
  function fresh(name){assert(!identifiers.has(name),'fresh helper namespace');return name;}
  function namespace(name){fresh(name);assert(state.namespace===null||state.namespace===name);state.namespace=name;state.imported=true;}
  function statement(node) {
    if(ts.isImportDeclaration(node)) {
      assert(external);assert(!state.imported,'one helper import');assert(ts.isStringLiteral(node.moduleSpecifier)&&node.moduleSpecifier.text==='tslib');
      if(blue){assert(ts.isNamespaceImport(node.importClause.namedBindings));namespace(node.importClause.namedBindings.name.text);exact(node,`import * as ${state.namespace} from 'tslib';`);}
      else{exact(node,`import {${member}} from 'tslib';`);state.imported=true;}
      generated.push(node);return true;
    }
    if(!ts.isVariableStatement(node)||node.declarationList.declarations.length!==1)return false;
    const declaration=node.declarationList.declarations[0],init=declaration.initializer;
    if(!init||!ts.isCallExpression(init)||init.expression.getText()!=='require')return false;
    assert(external&&ts.isIdentifier(declaration.name));assert(!state.imported,'one helper require');namespace(declaration.name.text);
    exact(node,`var ${state.namespace}=require('tslib');`);generated.push(node);return true;
  }
  function finish(){assert.equal(state.imported,external,'complete helper import inventory');}
  return {state,statement,namespace,finish,external,fresh};
}
module.exports={create};
