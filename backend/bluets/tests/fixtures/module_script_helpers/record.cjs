// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs'), os = require('os'), path = require('path'), vm = require('vm');
const ts = require(path.join(__dirname, '../oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-script-helpers-'));
const entry = path.join(root, 'main.ts');
fs.writeFileSync(entry, 'class Base {} class Derived extends Base {} var result: boolean = new Derived() instanceof Base;');
const observations = [];
for (const module of ['CommonJS', 'AMD', 'UMD']) for (const imports of [false, true]) for (const omit of [false, true]) {
 const options = {target:ts.ScriptTarget.ES5, lib:['lib.es2020.d.ts'], module:ts.ModuleKind[module], importHelpers:imports, noEmitHelpers:omit, declaration:true};
 const host = ts.createCompilerHost(options), outputs = {};
 host.writeFile = (name, text) => outputs[path.basename(name)] = text;
 const program = ts.createProgram([entry], options, host);
 const diagnostics = ts.getPreEmitDiagnostics(program).map(x => x.code);
 program.emit();
 const context = {__extends(child, parent) {Object.setPrototypeOf(child, parent); child.prototype = Object.create(parent.prototype); child.prototype.constructor = child;}};
 vm.runInNewContext(outputs['main.js'], context);
 observations.push({module,imports,omit,diagnostics,result:context.result,importsRuntime:outputs['main.js'].includes('tslib'),wrapper:outputs['main.js'].includes('define('),declaration:outputs['main.d.ts']});
}
fs.rmSync(root, {recursive:true,force:true});
const assert = require('assert');
const reference = JSON.parse(fs.readFileSync(path.join(__dirname, 'reference.json'), 'utf8'));
assert.deepStrictEqual({typescript:ts.version,observations}, reference);
const matrix = observations.map(x => `modscript-${x.module.toLowerCase()}-${x.imports?'import':'inline'}-${x.omit?'omit':'emit'}/main.ts\taccept\n`).join('');
assert.strictEqual(fs.readFileSync(path.join(__dirname, 'script-helpers-checker-matrix.tsv'), 'utf8').replaceAll('\r\n','\n'), matrix);
console.log(JSON.stringify({typescript:ts.version,cases:observations.length,allAccepted:observations.every(x=>x.diagnostics.length===0),allResultsTrue:observations.every(x=>x.result===true),noRuntimeImports:observations.every(x=>!x.importsRuntime),noWrappers:observations.every(x=>!x.wrapper)}));
