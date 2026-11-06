// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The admitted preserved-proposal adapter operates only on emitted JavaScript.
const path=require('node:path'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../../..'),acorn=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs'))(process.env.BLUEICE_ACORN_ORACLE||'acorn');
function parse(ts,code,options) {
  const preserved=options.target==='ESNext'&&options.useDefineForClassFields;
  const text=preserved?ts.transpileModule(code,{fileName:'emitted.js',compilerOptions:{target:ts.ScriptTarget.ES2022,module:String(options.module).toLowerCase()==='commonjs'?ts.ModuleKind.CommonJS:ts.ModuleKind.ESNext,useDefineForClassFields:true}}).outputText:code;
  const source=ts.createSourceFile('emitted.js',text,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS),module=source.statements.some(node=>ts.isImportDeclaration(node)||ts.isExportDeclaration(node)||ts.isExportAssignment(node)||node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword));
  let edition=preserved?2022:options.target==='ESNext'?'latest':Number(options.target.slice(2));if(module&&edition===5)edition=2015;
  assert.equal(source.parseDiagnostics.length,0,'complete emitted JavaScript parses');acorn.parse(text,{ecmaVersion:edition,sourceType:module?'module':'script'});
  return {edition,adapter:preserved?'typescript@5.9.3 emitted-JavaScript-only ES2022 lowering':null};
}
module.exports={parse};
