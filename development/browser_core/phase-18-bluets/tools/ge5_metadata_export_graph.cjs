// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original export symbols are followed through their exact source modules.
const assert=require('node:assert/strict'),path=require('node:path'),capabilities=new WeakSet();
function filename(from,specifier){assert(specifier.startsWith('.'),'original relative metadata dependency');return path.posix.normalize(path.posix.join(path.posix.dirname(from),specifier)).replace(/\.js$/,'.ts');}
function create(ts,sources) {
  const modules=new Set(),hops=new Map(),leaves=new Set();
  function resolve(file,name,stack=new Set()) {
    const identity=file+'\0'+name;assert(!stack.has(identity),'original export symbol graph has no unresolved cycle');stack=new Set(stack).add(identity);
    const source=sources.get(file);assert(source,'original re-export dependency resolves to a preserved input module');modules.add(file);
    const direct=source.statements.filter(node=>(ts.isClassDeclaration(node)||ts.isTypeAliasDeclaration(node)||ts.isInterfaceDeclaration(node)||ts.isEnumDeclaration(node))&&node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword)&&(node.modifiers.some(modifier=>modifier.kind===ts.SyntaxKind.DefaultKeyword)?'default':node.name?.text)===name);
    const indirect=source.statements.filter(ts.isExportDeclaration).flatMap(declaration=>declaration.exportClause&&ts.isNamedExports(declaration.exportClause)?declaration.exportClause.elements.filter(specifier=>specifier.name.text===name).map(specifier=>({declaration,specifier})):[]);
    assert.equal(direct.length+indirect.length,1,'metadata type resolves to one original exported provider symbol');
    if(direct.length){leaves.add(source);return {source,name,declaration:direct[0]};}
    const {declaration,specifier}=indirect[0];assert(!declaration.isTypeOnly&&!specifier.isTypeOnly,'original runtime named re-export edge');
    const imported=(specifier.propertyName??specifier.name).text;
    if(!declaration.moduleSpecifier){const local=source.statements.filter(node=>ts.isClassDeclaration(node)&&node.name?.text===imported&&!node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.DeclareKeyword));assert.equal(local.length,1,'local export resolves its exact original class binding');if(!hops.has(identity)){const cap={source,file,name,declaration,specifier,dependency:null,target:file,imported,local:true};capabilities.add(cap);hops.set(identity,cap);}leaves.add(source);return {source,name,declaration:local[0],localName:imported};}
    assert(ts.isStringLiteral(declaration.moduleSpecifier),'original runtime named re-export edge');const dependency=declaration.moduleSpecifier.text,target=filename(file,dependency);
    if(!hops.has(identity)){const cap={source,file,name,declaration,specifier,dependency,target,imported};capabilities.add(cap);hops.set(identity,cap);}
    return resolve(target,imported,stack);
  }
  function complete(entry){assert.equal(new Set([entry,...modules]).size,sources.size,'every original input module belongs to the complete import/re-export/provider graph');assert.equal(leaves.size,1,'metadata types resolve to one exact provider source module');return {reexports:[...hops.values()],provider:[...leaves][0],modules:[...modules]};}
  return {resolve,complete};
}
module.exports={filename,create,valid:cap=>capabilities.has(cap)};
