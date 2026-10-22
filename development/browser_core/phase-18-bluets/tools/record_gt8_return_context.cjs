// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// G-T8 observations are produced only by pinned TypeScript, never BlueTS.
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const support = path.join(root, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/gt8_return_context');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
if (input.schema !== 1 || input.typescript !== ts.version) throw Error('Foreign G-T8 input');
const libraries = new Map(), base = path.join(root, '__gt8_return_context__');
function compile(files, entry = 'entry.ts', options = input.options) {
    const converted = ts.convertCompilerOptionsFromJson(options, root);
    if (converted.errors.length) throw Error('Invalid native options');
    const native = converted.options;
    const sources = new Map(Object.entries(files).map(([name, text]) => [path.join(base, name), text]));
    const host = ts.createCompilerHost(native), read = host.getSourceFile;
    const exists = host.fileExists, readFile = host.readFile, directoryExists = host.directoryExists;
    host.fileExists = file => sources.has(file) || exists(file);
    host.readFile = file => sources.get(file) ?? readFile(file);
    host.directoryExists = file => file === base || directoryExists(file);
    host.getSourceFile = (file, version, ...args) => {
        if (sources.has(file)) return ts.createSourceFile(file, sources.get(file), version, true);
        if (!libraries.has(file)) libraries.set(file, read(file, version, ...args));
        return libraries.get(file);
    };
    const declarations = {}, javascript = {};
    host.writeFile = (file, text) => {
        const name = path.relative(base, file).replaceAll(path.sep, '/');
        if (name.endsWith('.d.ts')) declarations[name] = text.replaceAll('\r\n', '\n');
        if (name.endsWith('.js')) javascript[name] = text;
    };
    const program = ts.createProgram([path.join(base, entry)], native, host);
    const describe = d => ({
        code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, '\n'),
        module: d.file ? path.relative(base, d.file.fileName).replaceAll(path.sep, '/') : null,
        start: d.start ?? null, length: d.length ?? null,
        related: (d.relatedInformation || []).map(describe),
    });
    const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program)).map(describe);
    if (!diagnostics.length) program.emit();
    return {diagnostics, declarations, javascript};
}
function source(form, verdict) {
    return '// UTF-16 control: 😀\n' + form.prefix + '\n' + form[verdict] + '\n';
}
function execute(javascript, invoke) {
    const run = spawnSync(process.execPath, ['--input-type=module'], {
        input: javascript + '\n' + invoke, encoding: 'utf8',
    });
    if (run.error) throw run.error;
    return {exit: run.status, stdout: run.stdout.replaceAll('\r\n', '\n'), stderr: run.stderr.replaceAll('\r\n', '\n')};
}
if (process.argv.includes('--outputs')) {
    const reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json'), 'utf8'));
    const accepted = reference.cases.filter(row => row.accepts);
    const outputs = JSON.parse(fs.readFileSync(0, 'utf8'));
    const resolve = require(path.join(support, 'resolve_package.cjs'));
    const acorn = require(resolve(process.env.BLUEICE_ACORN_ORACLE, 'acorn', 'dist/acorn.js'));
    if (acorn.version !== '8.15.0' || outputs.length !== accepted.length) throw Error('Foreign/incomplete outputs');
    for (let index = 0; index < outputs.length; index++) {
        const actual = outputs[index], expected = accepted[index];
        if (actual.id !== expected.id || JSON.stringify(actual.declarations) !== JSON.stringify(expected.declarations)) {
            throw Error(expected.id + ': non-exact declaration graph');
        }
        for (const js of Object.values(actual.javascript)) acorn.parse(js, {ecmaVersion: 2022, sourceType: 'module'});
        for (const consumer of Object.values(expected.consumers)) {
            const checked = compile({...actual.declarations, 'consumer.ts': consumer.source}, 'consumer.ts', {...expected.options, noEmit: true});
            if (JSON.stringify(checked.diagnostics) !== JSON.stringify(consumer.diagnostics)) throw Error(expected.id + ': consumer diagnostics changed');
        }
        const equality = "type Equal<A,B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false; type Assert<T extends true> = T; type Forward = Assert<Equal<typeof import('./blue.js'), typeof import('./native.js')>>; type Reverse = Assert<Equal<typeof import('./native.js'), typeof import('./blue.js')>>;";
        const files = {...actual.declarations, 'blue.d.ts': actual.declarations['entry.d.ts'], 'native.d.ts': expected.declarations['entry.d.ts'], 'consumer.ts': equality};
        if (compile(files, 'consumer.ts', {...expected.options, noEmit: true}).diagnostics.length) throw Error(expected.id + ': bidirectional declaration mismatch');
        if (expected.invoke && JSON.stringify(execute(actual.javascript['entry.js'], expected.invoke)) !== JSON.stringify(expected.runtime)) throw Error(expected.id + ': runtime mismatch');
    }
    console.log('Verified ' + outputs.length + ' exact declaration graphs, consumers and syntax outputs');
    process.exit(0);
}
const cases = [];
for (const form of input.forms) for (const verdict of ['accept', 'reject']) {
    const options = {...input.options, ...form.options};
    const files = {...form.files, 'entry.ts': source(form, verdict)};
    const observed = compile(files, 'entry.ts', options);
    const accepts = observed.diagnostics.length === 0;
    if (accepts !== (verdict === 'accept')) throw Error(form.id + '-' + verdict + ': unexpected native verdict ' + JSON.stringify(observed.diagnostics));
    const row = {id: form.id + '-' + verdict, area: form.area, files, options, accepts, diagnostics: observed.diagnostics};
    if (accepts) {
        row.declarations = observed.declarations;
        row.consumers = {};
        for (const kind of ['accept', 'reject']) {
            const text = '// UTF-16 consumer: 😀\n' + form['consumer_' + kind] + '\n';
            const {diagnostics} = compile({...observed.declarations, 'consumer.ts': text}, 'consumer.ts', {...options, noEmit: true});
            if ((diagnostics.length === 0) !== (kind === 'accept')) throw Error(row.id + ': unexpected consumer ' + kind + ': ' + JSON.stringify(diagnostics));
            row.consumers[kind] = {source: text, diagnostics};
        }
        if (form.invoke) {
            row.invoke = form.invoke;
            row.runtime = execute(observed.javascript['entry.js'], form.invoke);
            if (row.runtime.exit !== 0) throw Error(row.id + ': native runtime failed ' + JSON.stringify(row.runtime));
        }
    }
    cases.push(row);
}
for (const control of input.budget_controls) {
    if (compile({'entry.ts': control.source}).diagnostics.length) throw Error(control.id + ': native budget control rejected');
}
for (const control of input.owner_controls) {
    const files = {...cases[0].declarations, 'consumer.ts': control.source};
    if (compile(files, 'consumer.ts', {...input.options, noEmit: true}).diagnostics.length) throw Error(control.id + ': native policy source rejected');
}
const text = JSON.stringify({schema: 1, typescript: ts.version, cases, budget_controls: input.budget_controls, owner_controls: input.owner_controls}, null, 2) + '\n';
const file = path.join(directory, 'reference.json');
if (process.env.BLUEICE_WRITE_GT8_RETURN_CONTEXT === '1') fs.writeFileSync(file, text);
else if (fs.readFileSync(file, 'utf8').replaceAll('\r\n', '\n') !== text) throw Error('G-T8 native observations changed');
console.log('Verified ' + cases.length + ' native G-T8 observations, ' + input.budget_controls.length + ' budget controls and ' + input.owner_controls.length + ' declaration owner control');
