// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs');
const path = require('path');
const {spawnSync} = require('child_process');
const root = path.resolve(__dirname, '../../../..');
const corpus = path.join(root, 'backend/bluets/tests/fixtures/typescript_oracle');
const support = path.join(corpus, '../oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require(path.join(support, 'load_acorn.cjs'))();
if (ts.version !== '5.9.3' || acorn.version !== '8.15.0') throw new Error('Unpinned oracle');
const settings = JSON.parse(fs.readFileSync(path.join(corpus,'targets-reference.json'),'utf8'));
const os = require('os');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-target-record-'));
const library = path.dirname(ts.getDefaultLibFilePath({}));
const parsedLibraries = new Map();
function host(options) {
  const result = ts.createCompilerHost(options);
  const read = result.getSourceFile;
  result.getSourceFile = (file,version,onError,createNew) => {
    if (path.dirname(file) !== library) return read(file,version,onError,createNew);
    const key = `${file}:${JSON.stringify(version)}`;
    if (!parsedLibraries.has(key)) parsedLibraries.set(key,read(file,version,onError,createNew));
    return parsedLibraries.get(key);
  };
  return result;
}
function diagnostic(item) {
  const result = {code:item.code,message:ts.flattenDiagnosticMessageText(item.messageText,'\n')};
  if (item.file && item.start !== undefined) {
    const position = item.file.getLineAndCharacterOfPosition(item.start);
    result.module = path.dirname(item.file.fileName)===library?`<typescript-lib>/${path.basename(item.file.fileName)}`:path.basename(item.file.fileName);
    result.position = {line:position.line+1,column:position.character+1,length:item.length || 0};
  }
  result.related = (item.relatedInformation || []).map(diagnostic);
  return result;
}
const cases = [];
for (const item of settings.cases) {
  const original = path.join(corpus,item.entry);
  const directory = path.join(work,path.dirname(item.entry));
  fs.mkdirSync(directory,{recursive:true});
  fs.copyFileSync(original,path.join(directory,'main.ts'));
  const entry = path.join(directory,'main.ts');
  const parsed = ts.parseCommandLine(item.flags);
  if (parsed.errors.length) throw new Error(JSON.stringify(parsed.errors.map(diagnostic)));
  const options = {...parsed.options,noEmit:true};
  const program = ts.createProgram([entry],options,host(options));
  const errors = ts.getPreEmitDiagnostics(program).filter(d=>d.category===ts.DiagnosticCategory.Error);
  const result = {entry:item.entry,target:item.target,form:item.form,accepts:errors.length===0,flags:item.flags,sources:['main.ts'],files:['main.ts'],runtime:errors.length===0,declaration:errors.length===0,first:errors.length?diagnostic(errors[0]):null,stdout:null,declaration_text:null,reference_helpers:[]};
  if (!errors.length) {
    const outDir = path.join(directory,'out');
    fs.writeFileSync(path.join(directory,'package.json'),JSON.stringify({type:'commonjs'})+'\n');
    const emitOptions = {...parsed.options,noEmit:false,declaration:true,noEmitOnError:true,newLine:ts.NewLineKind.LineFeed,outDir};
    const emitProgram = ts.createProgram([entry],emitOptions,host(emitOptions));
    const emitErrors = ts.getPreEmitDiagnostics(emitProgram).filter(d=>d.category===ts.DiagnosticCategory.Error);
    const emitted = emitProgram.emit();
    result.emit_diagnostics = [...emitErrors,...emitted.diagnostics].map(diagnostic);
    result.emit_skipped = emitted.emitSkipped;
    if (result.emit_diagnostics.length || emitted.emitSkipped) throw new Error(JSON.stringify(result));
    const js = fs.readFileSync(path.join(outDir,'main.js'),'utf8');
    const declaration = fs.readFileSync(path.join(outDir,'main.d.ts'),'utf8');
    const edition = item.target==='ESNext'?'latest':item.target==='ES5'?5:Number(item.target.slice(2));
    acorn.parse(js,{ecmaVersion:edition,sourceType:'script'});
    const run = spawnSync(process.execPath,[path.join(outDir,'main.js')],{encoding:'utf8',timeout:30000});
    if (run.error || run.status!==0) throw new Error(JSON.stringify({entry:item.entry,status:run.status,error:String(run.error),stderr:run.stderr}));
    result.declaration_text = declaration;
    result.stdout = run.stdout;
    result.reference_helpers = [...new Set([...js.matchAll(/(?:var|function)\s+(__[A-Za-z][A-Za-z0-9_]*)\b/g)].map(m=>m[1]))].sort();
  }
  cases.push(result);

}
const record = {version:ts.version,targets:settings.targets,forms:settings.forms,cases};
for (const item of record.cases) { delete item.emit_diagnostics; delete item.emit_skipped; }
const text = JSON.stringify(record,null,2)+'\n';
const destination = path.join(corpus,'targets-reference.json');
const matrix = cases.map(c=>`${c.entry}\t${c.accepts?'accept':'reject'}\n`).join('');
if (process.env.BLUEICE_WRITE_TARGETS_MATRIX === '1') {
    fs.writeFileSync(destination,text);
    fs.writeFileSync(path.join(corpus,'targets-checker-matrix.tsv'),matrix);
} else if (fs.readFileSync(destination,'utf8').replaceAll('\r\n','\n') !== text
    || fs.readFileSync(path.join(corpus,'targets-checker-matrix.tsv'),'utf8').replaceAll('\r\n','\n') !== matrix) {
    throw new Error('Target reference observations changed');
}
fs.rmSync(work,{recursive:true,force:true});
process.stdout.write(JSON.stringify({cases:cases.length,accept:cases.filter(c=>c.accepts).length,reject:cases.filter(c=>!c.accepts).length,runtime:cases.filter(c=>c.runtime).length})+'\n');
