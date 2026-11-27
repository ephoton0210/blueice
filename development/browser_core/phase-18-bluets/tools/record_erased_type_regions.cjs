// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

'use strict';
// Capture Native before implementation. Never refresh an existing authority.
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const repo = path.resolve(__dirname, '../../../..');
const fixture = path.join(repo, 'backend/bluets/tests/fixtures/erased_type_regions');
const root = path.resolve(process.argv[2]);
const tsRoot = path.resolve(process.env.BLUEICE_TYPESCRIPT_ROOT);
const ts = require(tsRoot);
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const write = (name, value) => fs.writeFileSync(name, JSON.stringify(value, null, 2) + '\n', {flag:'wx'});
const implementation = path.join(tsRoot, 'lib/typescript.js');
assert.equal(ts.version, '5.9.3');
assert.equal(sha(fs.readFileSync(implementation)), '3ae902c92cc44dace175c0e69e13a4b0899f6983c6121d76b9ab8dd5795e7675');
assert(!fs.existsSync(root), 'retain every previous recorder attempt');
assert(!fs.existsSync(path.join(fixture, 'FROZEN.json')), 'Native authority already frozen');
fs.mkdirSync(root, {recursive:true});
const input = JSON.parse(fs.readFileSync(path.join(fixture, 'cases.json')));
const libraries = {};
function diagnostics(values, base) {
    return values.map(d => {
        const pos = d.file && d.start !== undefined ? d.file.getLineAndCharacterOfPosition(d.start) : null;
        return {code:d.code, message:ts.flattenDiagnosticMessageText(d.messageText, '\n'),
            file:d.file ? path.relative(base,d.file.fileName).split(path.sep).join('/') : null,
            position:pos ? {line:pos.line+1,column:pos.character+1,length:d.length} : null,
            relatedInformation:diagnostics(d.relatedInformation || [],base)};
    });
}
function run(dir, files, options) {
    fs.mkdirSync(dir, {recursive:true});
    for (const [name,text] of Object.entries(files)) fs.writeFileSync(path.join(dir,name),text,{flag:'wx'});
    const converted = ts.convertCompilerOptionsFromJson(options,dir);
    assert.deepEqual(converted.errors, []);
    const outputs = {};
    const host = ts.createCompilerHost(converted.options);
    host.writeFile = (name,text) => { outputs[path.relative(path.join(dir,'out'),name).split(path.sep).join('/')] = text; };
    const program = ts.createProgram(Object.keys(files).map(name=>path.join(dir,name)),converted.options,host);
    const errors = ts.getPreEmitDiagnostics(program);
    const emitted = program.emit();
    for (const file of program.getSourceFiles()) if (file.fileName.startsWith(path.join(tsRoot,'lib')+path.sep))
        libraries[path.basename(file.fileName)] = sha(fs.readFileSync(file.fileName));
    const row = {accepts:!errors.some(d=>d.category===ts.DiagnosticCategory.Error),
        diagnostics:diagnostics(errors,dir),emitSkipped:emitted.emitSkipped,outputs};
    write(path.join(dir,'native.json'),row);
    return row;
}
const rows = [], consumers = [], executions = [];
for (const entry of input.cases) for (const profile of input.profiles) {
    const id = entry.id+'/'+profile.id;
    const dir = path.join(root,id);
    const files = {'main.ts':entry.source,...(entry.files||{})};
    const result = run(dir,files,profile.compilerOptions);
    rows.push({id,form:entry.id,profile:profile.id,...result});
    if (result.accepts) {
        assert.equal(typeof entry.positive,'string',id+': accepted fixture needs authored consumers');
        for (const polarity of ['positive','negative']) {
            const consumerFiles = Object.fromEntries(Object.entries(result.outputs).filter(([name])=>name.endsWith('.d.ts')));
            consumerFiles['consumer.ts'] = 'import {result} from "./main.js";\n'+entry[polarity];
            const checked = run(path.join(dir,'consumer-'+polarity),consumerFiles,
                {...profile.compilerOptions,declaration:false,declarationMap:false,sourceMap:false,noEmit:true});
            assert.equal(checked.accepts,polarity==='positive',id+': '+polarity+' consumer');
            consumers.push({id,polarity,accepts:checked.accepts,diagnostics:checked.diagnostics});
        }
        if (entry.runtime) {
            const runDir = path.join(dir,'runtime');fs.mkdirSync(runDir);
            fs.writeFileSync(path.join(runDir,'package.json'),JSON.stringify({type:profile.compilerOptions.module==='CommonJS'?'commonjs':'module'}));
            for (const [name,text] of Object.entries(result.outputs).filter(([name])=>name.endsWith('.js')))
                fs.writeFileSync(path.join(runDir,name),text);
            const source = 'const m = await import('+JSON.stringify(path.join(runDir,'main.js'))+');\n'+entry.runtime;
            fs.writeFileSync(path.join(runDir,'execute.mjs'),source);
            const ran = cp.spawnSync(process.execPath,['--no-maglev',path.join(runDir,'execute.mjs')],{encoding:'utf8'});
            fs.writeFileSync(path.join(runDir,'stdout'),ran.stdout);fs.writeFileSync(path.join(runDir,'stderr'),ran.stderr);
            assert.equal(ran.status,0,id+': Native execution');
            executions.push({id,exitCode:ran.status,stdout:ran.stdout,stderr:ran.stderr});
        }
    } else {
        assert.deepEqual(result.outputs,{},id+': noEmitOnError');
        assert.equal(entry.positive,undefined,id+': an authored acceptance unexpectedly rejects');
    }
    process.stdout.write(JSON.stringify({id,accepts:result.accepts,codes:result.diagnostics.map(d=>d.code)})+'\n');
}
const reference = {nativeVersion:ts.version,implementationSha256:sha(fs.readFileSync(implementation)),libraries,
    observations:rows.length,cases:rows,consumers,executions};
write(path.join(fixture,'reference.json'),reference);
write(path.join(fixture,'FROZEN.json'),{schemaVersion:1,frozenBeforeProductionChanges:true,
    acceptedParent:input.acceptedParent,nativeVersion:ts.version,implementationSha256:reference.implementationSha256,libraries,
    sha256:{'cases.json':sha(fs.readFileSync(path.join(fixture,'cases.json'))),
        'reference.json':sha(fs.readFileSync(path.join(fixture,'reference.json')))},
    recorderSha256:sha(fs.readFileSync(__filename)),forms:input.cases.length,profiles:input.profiles.length,
    observations:rows.length,accepted:rows.filter(row=>row.accepts).length,rejected:rows.filter(row=>!row.accepts).length,
    consumers:consumers.length,executions:executions.length});
