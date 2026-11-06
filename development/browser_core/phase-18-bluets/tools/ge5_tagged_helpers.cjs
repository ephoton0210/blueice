// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete site-constructor/import ABIs; generated helpers own no source.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const native=`var __makeTemplateObject=(this&&this.__makeTemplateObject)||function(cooked,raw){if(Object.defineProperty){Object.defineProperty(cooked,'raw',{value:raw});}else{cooked.raw=raw;}return cooked;};`;
function create(ts,view,options,blue,identifiers,exact,generated,helpers) {
  const imports=require('./ge5_helper_imports.cjs').create(ts,view,options,blue,identifiers,exact,generated,'__makeTemplateObject');
  const {state,external,namespace}=imports;let definitions=0;
  function statement(node) {
    if(imports.statement(node))return true;
    if(ts.isVariableStatement(node)&&node.declarationList.declarations.length===1&&node.declarationList.declarations[0].name.getText()==='__makeTemplateObject') {
      assert(!blue&&options.target==='ES5'&&!options.importHelpers&&!options.noEmitHelpers);exact(node,native);helpers.set('__makeTemplateObject','template');definitions++;generated.push(node);return true;
    }
    if(!ts.isFunctionDeclaration(node)||!/^__blueice_target_template_object_\d+$/.test(node.name?.text))return false;
    assert(blue&&options.target==='ES5'&&!options.importHelpers&&!options.noEmitHelpers);imports.fresh(node.name.text);assert(!helpers.has(node.name.text));
    const source=fs.readFileSync(path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets/es5/templates/tagged.v1.js'),'utf8').replaceAll('__blueice_target_template_object',node.name.text);
    exact(node,source);helpers.set(node.name.text,'template');definitions++;generated.push(node);return true;
  }
  function finish(){imports.finish();assert.equal(definitions,options.target==='ES5'&&!options.importHelpers&&!options.noEmitHelpers?1:0,'one complete site constructor');}
  return {state,external,namespace,statement,finish};
}
module.exports={create};
