// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Retained class syntax has an exact ordered token contract. Only formatting
// changes; every UTF-16 owner and every surviving physical gap follows Native.
const assert=require('node:assert/strict'),accounting=require('./ge5_class_accounting.cjs');
function compare(ts,native,actual,options,originalText) {
  assert.notEqual(options.target,'ES5','retained class target');
  assert.deepEqual(actual.decoded.metadata,native.decoded.metadata);
  if(actual.code===native.code)assert.deepEqual(actual.decoded,native.decoded,'identical output requires every decoded entry');
  const original=ts.createSourceFile('original.ts',originalText,ts.ScriptTarget.Latest,true),classes=[];
  for(const declaration of original.statements.filter(ts.isClassDeclaration)) {
    const wanted=native.source.statements.filter(node=>ts.isClassDeclaration(node)&&node.name?.text===declaration.name.text),observed=actual.source.statements.filter(node=>ts.isClassDeclaration(node)&&node.name?.text===declaration.name.text);
    assert.equal(wanted.length,1);assert.equal(observed.length,1);
    const n={native:true,node:wanted[0]},a={native:true,node:observed[0]};
    const declarationChain=require('./ge5_class_declaration_chain.cjs').compare(ts,native,wanted[0],n,actual,observed[0],a);
    classes.push({native:n,actual:a,declarationChain});
  }
  assert.equal(classes.length,2,'complete source class inventory');
  const contract=accounting.create(ts,native,actual,[originalText]);
  contract.pair(native.tokens(native.source),actual.tokens(actual.source),'complete retained class module');
  contract.finish(classes);
  return {classes,actual:{sourceNodes:[...actual.source.statements]},helpers:new Map()};
}
module.exports={compare};
