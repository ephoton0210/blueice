// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The frozen owner runtime mappings are test harness policy, not compiler emit.
const fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {spawnSync}=require('node:child_process');
const {pathToFileURL}=require('node:url');
function owned(root,name){
  const target=path.resolve(root,name);
  if(!target.startsWith(root+path.sep)||name.includes('\\'))throw Error('unowned fixture path: '+name);
  return target;
}
function write(root,files){for(const [name,text]of Object.entries(files)){
  const target=owned(root,name);fs.mkdirSync(path.dirname(target),{recursive:true});fs.writeFileSync(target,text);
}}
function imports(ts,files){return Object.fromEntries(Object.entries(files).filter(([n])=>/\.(?:js|mjs|cjs)$/.test(n)).sort().map(([name,text])=>{
  const found=[],source=ts.createSourceFile(name,text,ts.ScriptTarget.ES2022,true,ts.ScriptKind.JS);
  function visit(node){
    if(ts.isImportDeclaration(node))found.push({specifier:node.moduleSpecifier.text,names:node.importClause?[node.importClause.name?.text,...(node.importClause.namedBindings?ts.isNamespaceImport(node.importClause.namedBindings)?['*']:node.importClause.namedBindings.elements.map(n=>n.propertyName?.text||n.name.text):[])].filter(Boolean):[]});
    if(ts.isCallExpression(node)&&ts.isIdentifier(node.expression)&&node.expression.text==='require'&&node.arguments.length===1&&ts.isStringLiteral(node.arguments[0]))found.push({specifier:node.arguments[0].text});
    ts.forEachChild(node,visit);
  }visit(source);return[name,found];
}));}
function execute(row,outputs){
  const root=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'bluets-gm1-run-')));
  try{
    write(root,row.files);write(root,outputs);
    write(root,{'out/package.json':JSON.stringify({type:row.runtimeModule==='CommonJS'?'commonjs':'module'})});
    const entry='out/'+row.entry.replace(/\.(?:ts|js)$/i,'.js').replace(/\.mts$/i,'.mjs').replace(/\.cts$/i,'.cjs');
    const mappings=Object.fromEntries(Object.entries(row.runtimeMappings).map(([key,value])=>[
      key.includes('|')?pathToFileURL(owned(root,key.split('|')[0])).href+'|'+key.split('|')[1]:key,
      pathToFileURL(owned(root,value)).href
    ]));
    // A single fixed hook is shared by native/BlueTS runs. No compiler result
    // may select a mapping, and the selected source graph is checked separately.
    const script=`const {registerHooks}=require('node:module');const mappings=${JSON.stringify(mappings)};registerHooks({resolve(s,c,next){const target=mappings[c.parentURL+'|'+s]||mappings[s];return target?{url:target,shortCircuit:true}:next(s,c);}});\n`+
      (row.runtimeModule==='CommonJS'?`const m=require(${JSON.stringify(owned(root,entry))});process.stdout.write(JSON.stringify(m.result));`:
      `import(${JSON.stringify(pathToFileURL(owned(root,entry)).href)}).then(m=>process.stdout.write(JSON.stringify(m.result)));`);
    const conditions=(row.options.customConditions||[]).map(c=>'--conditions='+c);
    const result=spawnSync(process.execPath,[...conditions,'-e',script],{encoding:'utf8'});
    if(result.error)throw result.error;
    return {exit:result.status,stdout:result.stdout,stderr:result.stderr.replaceAll(root,'<root>')};
  }finally{fs.rmSync(root,{recursive:true,force:true});}
}
module.exports={owned,write,imports,execute};
