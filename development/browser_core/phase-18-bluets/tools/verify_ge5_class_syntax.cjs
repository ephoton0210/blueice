// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Both complete products must parse at the configured JavaScript edition.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),selection=require('./ge5_class_cases.cjs');
const root=path.resolve(__dirname,'../../../..'),input=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/cases.json')),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const acorn=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs'))(process.env.BLUEICE_ACORN_ORACLE||'acorn');
const rows=JSON.parse(fs.readFileSync(0,'utf8')),identity=selection.verify(root,input,reference,rows),verified=[];
for(const row of rows)for(const [name,map]of Object.entries(reference.cases[row.index].maps)) {
  if(map.kind!=='javascript')continue;
  const edition=row.options.target==='ESNext'?'latest':Number(row.options.target.slice(2));assert(edition===5||edition>=2015&&edition<=2022);
  for(const [producer,code]of [['Native',reference.blobs[reference.cases[row.index].outputs[map.output]]],['Blue',row.outputs[map.output]]]) {
    const source=ts.createSourceFile(map.output,code,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
    const module=source.statements.some(node=>ts.isImportDeclaration(node)||ts.isExportDeclaration(node)||ts.isExportAssignment(node)||node.modifiers?.some(modifier=>modifier.kind===ts.SyntaxKind.ExportKeyword));
    acorn.parse(code,{ecmaVersion:edition,sourceType:module?'module':'script'});verified.push({index:row.index,map:name,producer,edition,codeSha256:require('./ge5_inputs.cjs').sha256(code)});
  }
}
assert.equal(rows.length,661);assert.equal(verified.length,1424);const report={...identity,configurations:rows.length,programs:verified.length,verified};
if(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY){fs.mkdirSync(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,{recursive:true});fs.writeFileSync(path.join(process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY,'class-syntax.json'),JSON.stringify(report,null,2)+'\n');}
console.log(JSON.stringify(report,null,2));
