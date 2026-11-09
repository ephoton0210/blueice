// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Freeze configuration diagnostics and public state paths before implementation.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const cp = require('node:child_process');
const repo = path.resolve(__dirname, '../../../..');
const directory = path.join(repo, 'backend/bluets/tests/fixtures/gm2_incremental_options');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
const resolve = require(path.join(repo, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const ts = require(resolve(process.env.BLUEICE_BLUETSC_ORACLE, 'typescript', 'lib/typescript.js'));
assert.equal(ts.version, '5.9.3');
const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'gm2-config-native-'));
function diagnostics(items) {
    return ts.sortAndDeduplicateDiagnostics(items).map(d => ({
        code:d.code, message:ts.flattenDiagnosticMessageText(d.messageText, '\n'),
        module:d.file ? path.relative(root, d.file.fileName).replaceAll(path.sep, '/') : null,
        start:d.start ?? null, length:d.length ?? null,
        related:(d.relatedInformation || []).map(r => ({code:r.code,
            message:ts.flattenDiagnosticMessageText(r.messageText, '\n')}))
    }));
}
try {
    const cases = input.cases.map(form => {
        fs.rmSync(root, {recursive:true, force:true}); fs.mkdirSync(root);
        for (const [name, source] of Object.entries(form.files)) {
            fs.mkdirSync(path.dirname(path.join(root, name)), {recursive:true});
            fs.writeFileSync(path.join(root, name), source);
        }
        const parsed = ts.getParsedCommandLineOfConfigFile(path.join(root, 'tsconfig.json'), {},
            {...ts.sys, onUnRecoverableConfigFileDiagnostic:d => {throw Error(d.messageText);}});
        const program = ts.createIncrementalProgram({rootNames:parsed.fileNames, options:parsed.options,
            configFileParsingDiagnostics:parsed.errors});
        const observed = diagnostics([...parsed.errors, ...ts.getPreEmitDiagnostics(program.getProgram())]);
        assert.equal(observed.length === 0, form.accepts, form.id);
        const outputs = {};
        if (form.accepts) program.emit(undefined, (name, source) => {
            outputs[path.relative(root, name).replaceAll(path.sep, '/')] = source;
        });
        const states = Object.keys(outputs).filter(name => name.endsWith('.tsbuildinfo'));
        assert(states.length <= 1);
        const declaration = form.declarationPath ? outputs[form.declarationPath] : Object.entries(outputs).find(([name]) => name.endsWith('.d.ts'))?.[1] ?? null;
        const declarations = form.runtimePath ? Object.fromEntries(Object.entries(outputs).filter(([name]) => name.endsWith('.d.ts'))) : null;
        if (form.accepts) {
            for (const [name, source] of Object.entries(outputs)) {
                fs.mkdirSync(path.dirname(path.join(root, name)), {recursive:true});
                fs.writeFileSync(path.join(root, name), source);
            }
            const javascript = Object.entries(outputs).find(([name]) => name.endsWith('.js'))[1];
            const script = form.runtimePath ? `console.log(require(${JSON.stringify(path.join(root, form.runtimePath))}).value());` : javascript + '\nconsole.log(exports.value());';
            const runtime = cp.spawnSync(process.execPath, ['-e', script], {encoding:'utf8'});
            assert.equal(runtime.status, 0); assert.equal(runtime.stdout.trim(), '42');
        }
        return {...form, diagnostics:observed, statePath:states[0] ?? null, declaration, ...(declarations ? {declarations} : {})};
    });
    const text = JSON.stringify({schema:1, typescript:ts.version, cases, ownerControls:input.ownerControls}, null, 2) + '\n';
    const file = path.join(directory, 'reference.json');
    if (process.env.BLUEICE_WRITE_GM2_INCREMENTAL_OPTIONS === '1') fs.writeFileSync(file, text);
    else assert.equal(fs.readFileSync(file, 'utf8').replaceAll('\r\n', '\n'), text);
    console.log(`Verified ${cases.length} Native incremental configuration observations`);
} finally { fs.rmSync(root, {recursive:true, force:true}); }
