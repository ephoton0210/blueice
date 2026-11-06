// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Complete versioned helper bodies and their fresh dependency graph.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
function validate(ts,view,nodes,sourceNames) {
  if(!nodes.length)return;
  const generator=nodes.filter(node=>/^__blueice_target_(?:generator|delegate_iterator|delegate_step)_\d+$/.test(node.name?.text));
  nodes=nodes.filter(node=>!generator.includes(node));
  if(generator.length)validateGenerator(ts,view,generator,sourceNames);
  if(!nodes.length)return;
  assert.equal(nodes.length,3,'complete iterator-read helper group');
  const roles=new Map();
  for(const node of nodes) {
    assert(ts.isFunctionDeclaration(node)&&node.name);
    const name=node.name.text,role=/^__blueice_target_(binding_read|binding_iterator|binding_step)_\d+$/.exec(name)?.[1];
    assert(role,'structurally identified binding helper');assert(!sourceNames.has(name),'fresh helper binding');assert(![...roles.values()].includes(role),'unique helper dependency');roles.set(name,role);
  }
  const aliases=new Map([...roles].map(([name,role])=>[name,'$'+role]));
  const wantedAliases=new Map([['__blueice_target_binding_read','$binding_read'],['__blueice_target_binding_iterator','$binding_iterator'],['__blueice_target_binding_step','$binding_step'],['__blueice_target_iterator','$binding_iterator'],['__blueice_target_step_result','$binding_step']]);
  const directory=path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets');
  const source=ts.createSourceFile('binding-abi.js',fs.readFileSync(path.join(directory,'binding_helpers.v1.js'),'utf8')+fs.readFileSync(path.join(directory,'iteration_helpers.v1.js'),'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
  function words(node,source,alias){const result=[];function visit(node){if(ts.isToken(node)){if(node.kind!==ts.SyntaxKind.EndOfFileToken)result.push(alias.get(node.getText(source))||node.getText(source));return;}for(const child of node.getChildren(source))visit(child);}visit(node);return result;}
  assert.deepEqual(nodes.map(node=>words(node,view.source,aliases)),source.statements.map(node=>words(node,source,wantedAliases)),'complete binding iterator ABI');
}
function validateGenerator(ts,view,nodes,sourceNames) {
  assert.equal(nodes.length,3,'complete generator helper dependency group');const aliases=new Map();
  for(const node of nodes){assert(ts.isFunctionDeclaration(node));const match=/^__blueice_target_(generator|delegate_iterator|delegate_step)_\d+$/.exec(node.name.text);assert(match&&!sourceNames.has(node.name.text));aliases.set(node.name.text,'$'+match[1]);}
  assert.equal(new Set(aliases.values()).size,3);
  const directory=path.resolve(__dirname,'../../../../backend/bluets/src/emitter/targets'),source=ts.createSourceFile('generator-abi.js',fs.readFileSync(path.join(directory,'generator_helpers.v1.js'),'utf8')+fs.readFileSync(path.join(directory,'generator_delegation_helpers.v1.js'),'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
  const wanted=new Map([['__blueice_target_generator','$generator'],['__blueice_target_iterator','$delegate_iterator'],['__blueice_target_step_result','$delegate_step']]);
  function tokens(node,source,aliases){const result=[];function visit(node){if(ts.isToken(node)){if(node.kind!==ts.SyntaxKind.EndOfFileToken)result.push(aliases.get(node.getText(source))||node.getText(source));return;}for(const child of node.getChildren(source))visit(child);}visit(node);return result;}
  assert.deepEqual(nodes.map(node=>tokens(node,view.source,aliases)),source.statements.map(node=>tokens(node,source,wanted)),'complete generator helper ABI');
}
module.exports={validate};
