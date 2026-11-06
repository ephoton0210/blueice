// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Reviewed helper algorithms and import ABIs, independent of emitted map grants.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const nativeAppend=`var __spreadArray=(this&&this.__spreadArray)||function(to,from,pack){
if(pack||arguments.length===2)for(var i=0,l=from.length,ar;i<l;i++){if(ar||!(i in from)){if(!ar)ar=Array.prototype.slice.call(from,0,i);ar[i]=from[i];}}
return to.concat(ar||Array.prototype.slice.call(from));};`;
function create(ts,view,options,blue,identifiers,exact,generated,helpers) {
  const imports=require('./ge5_helper_imports.cjs').create(ts,view,options,blue,identifiers,exact,generated,'__spreadArray');
  const {state,external,fresh,namespace}=imports;state.groups=new Map();
  function statement(node) {
    if(imports.statement(node))return true;
    if(ts.isVariableStatement(node)&&node.declarationList.declarations.length===1) {
      const declaration=node.declarationList.declarations[0];
      if(ts.isIdentifier(declaration.name)&&declaration.name.text==='__spreadArray') {
        assert(!blue&&options.target==='ES5'&&!options.importHelpers&&!options.noEmitHelpers);exact(node,nativeAppend);helpers.set('__spreadArray','append');generated.push(node);return true;
      }
    }
    if(!ts.isFunctionDeclaration(node)||!/^__blueice_target_spread_/.test(node.name?.text))return false;
    assert(blue&&options.target==='ES5'&&!options.importHelpers&&!options.noEmitHelpers);
    const match=/^__blueice_target_spread_(array|call|construct|iterator|step)_(\d+)$/.exec(node.name.text);assert(match,'reviewed spread helper');fresh(node.name.text);
    const kinds=['array','call','construct','iterator','step'],base=Number(match[2])-kinds.indexOf(match[1]);assert(base>=0);
    const names=new Map(kinds.map((kind,index)=>[kind,`__blueice_target_spread_${kind}_${base+index}`]));
    const directory=path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets');
    const source=ts.createSourceFile('helper-spec.js',[fs.readFileSync(path.join(directory,'spread_helpers.v1.js'),'utf8'),fs.readFileSync(path.join(directory,'iteration_helpers.v1.js'),'utf8')].join('\n')
      .replaceAll('__blueice_target_spread_array',names.get('array')).replaceAll('__blueice_target_spread_call',names.get('call')).replaceAll('__blueice_target_spread_construct',names.get('construct'))
      .replaceAll('__blueice_target_iterator',names.get('iterator')).replaceAll('__blueice_target_step_result',names.get('step')),ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
    const wanted=source.statements.find(item=>item.name?.text===node.name.text);assert(wanted);exact(node,wanted.getText(source));
    if(!state.groups.has(base))state.groups.set(base,new Set());assert(!state.groups.get(base).has(match[1]));state.groups.get(base).add(match[1]);
    helpers.set(node.name.text,match[1]);generated.push(node);return true;
  }
  function finish() {
    imports.finish();
    for(const group of state.groups.values())assert.deepEqual([...group].sort(),['array','call','construct','iterator','step'].sort(),'whole coherent helper group');
    if(blue&&options.target==='ES5'&&!options.importHelpers&&!options.noEmitHelpers)assert.equal(state.groups.size,2,'one reviewed group for each spread lowering');
  }
  return {state,statement,namespace,finish,external};
}
module.exports={create};
