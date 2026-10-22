// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// G-T7 observations come exclusively from the pinned native compiler.
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const support = path.join(root, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/gt7_library_manifest');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
if (input.schema !== 1 || input.typescript !== ts.version) throw Error('Foreign G-T7 manifest');
const base = path.join(root, '__gt7_library_manifest__'), libraries = new Map();
function compile(files, entry, options) {
    const converted = ts.convertCompilerOptionsFromJson(options, root);
    if (converted.errors.length) throw Error('Invalid native options');
    const sources = new Map(Object.entries(files).map(([name, text]) => [path.join(base, name), text]));
    const host = ts.createCompilerHost(converted.options), read = host.getSourceFile;
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
    const program = ts.createProgram([path.join(base, entry)], converted.options, host);
    const describe = d => ({code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, '\n'),
        module: d.file ? (path.basename(d.file.fileName).startsWith('lib.') ? path.basename(d.file.fileName) : path.relative(base, d.file.fileName).replaceAll(path.sep, '/')) : null,
        start: d.start ?? null, length: d.length ?? null, related: (d.relatedInformation || []).map(describe)});
    const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program)).map(describe);
    if (!diagnostics.length) program.emit();
    return {diagnostics, declarations, javascript};
}
function execute(javascript, invoke) {
    const run = spawnSync(process.execPath, ['--input-type=module'], {input: javascript + '\n' + invoke, encoding: 'utf8'});
    if (run.error) throw run.error;
    return {exit: run.status, stdout: run.stdout.replaceAll('\r\n', '\n'), stderr: run.stderr.replaceAll('\r\n', '\n')};
}
if (process.argv.includes('--outputs')) {
    const reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json'), 'utf8'));
    const accepted = [...reference.cases, ...reference.profile_controls].filter(row => row.accepts);
    const outputs = JSON.parse(fs.readFileSync(0, 'utf8'));
    const resolve = require(path.join(support, 'resolve_package.cjs'));
    const acorn = require(resolve(process.env.BLUEICE_ACORN_ORACLE, 'acorn', 'dist/acorn.js'));
    if (acorn.version !== '8.15.0' || outputs.length !== accepted.length) throw Error('Foreign/incomplete outputs');
    for (let index = 0; index < outputs.length; index++) {
        const actual = outputs[index], expected = accepted[index];
        if (actual.id !== expected.id || JSON.stringify(actual.declarations) !== JSON.stringify(expected.declarations)) throw Error(expected.id + ': non-exact declarations');
        // ESM syntax itself needs ES2015 even when expressions are ES5-lowered.
        const edition = expected.options.target === 'ES5' ? 2015 : 2022;
        for (const js of Object.values(actual.javascript)) acorn.parse(js, {ecmaVersion: edition, sourceType: 'module'});
        for (const consumer of Object.values(expected.consumers || {})) {
            const checked = compile({...actual.declarations, 'consumer.ts': consumer.source}, 'consumer.ts', {...expected.options, noEmit: true});
            if (JSON.stringify(checked.diagnostics) !== JSON.stringify(consumer.diagnostics)) throw Error(expected.id + ': downstream diagnostics changed');
        }
        const equality = "type Equal<A,B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false; type Assert<T extends true> = T; type Forward = Assert<Equal<typeof import('./blue.js'), typeof import('./native.js')>>; type Reverse = Assert<Equal<typeof import('./native.js'), typeof import('./blue.js')>>;";
        const checked = compile({...actual.declarations, 'blue.d.ts': actual.declarations['entry.d.ts'], 'native.d.ts': expected.declarations['entry.d.ts'], 'consumer.ts': equality}, 'consumer.ts', {...expected.options, noEmit: true});
        if (checked.diagnostics.length) throw Error(expected.id + ': bidirectional declaration mismatch');
        if (expected.invoke && JSON.stringify(execute(actual.javascript['entry.js'], expected.invoke)) !== JSON.stringify(expected.runtime)) throw Error(expected.id + ': execution mismatch');
    }
    console.log('Verified ' + outputs.length + ' exact declaration graphs and their native consumers/syntax/execution');
    process.exit(0);
}
const cases = [], profiles = [];
for (const form of input.forms) for (const verdict of ['accept', 'reject']) {
    const files = {'entry.ts': '// UTF-16 control: 😀\n' + form.prefix + '\n' + form[verdict] + '\n'};
    const options = {...input.options, ...form.options};
    const observed = compile(files, 'entry.ts', options), accepts = observed.diagnostics.length === 0;
    if (accepts !== (verdict === 'accept')) throw Error(form.id + '-' + verdict + ': unexpected native verdict ' + JSON.stringify(observed.diagnostics));
    const row = {id: form.id + '-' + verdict, area: form.area, files, options, accepts, diagnostics: observed.diagnostics};
    if (accepts) {
        row.declarations = observed.declarations; row.consumers = {};
        for (const kind of ['accept', 'reject']) {
            const source = '// UTF-16 consumer: 😀\n' + form['consumer_' + kind] + '\n';
            const checked = compile({...observed.declarations, 'consumer.ts': source}, 'consumer.ts', {...options, noEmit: true});
            if ((checked.diagnostics.length === 0) !== (kind === 'accept')) throw Error(row.id + ': unexpected consumer ' + kind + ' ' + JSON.stringify(checked.diagnostics));
            row.consumers[kind] = {source, diagnostics: checked.diagnostics};
        }
        if (form.invoke) {
            row.invoke = form.invoke; row.runtime = execute(observed.javascript['entry.js'], form.invoke);
            if (row.runtime.exit !== 0) throw Error(row.id + ': native execution failed');
        }
    }
    cases.push(row);
}
for (const control of input.profile_controls) for (const eligible of [true, false]) {
    const options = {...input.options, target: 'ES5', lib: [eligible ? control.edition : control.below]};
    const files = {'entry.ts': '// UTF-16 profile: 😀\n' + control.source + '\n'};
    const observed = compile(files, 'entry.ts', options), accepts = observed.diagnostics.length === 0;
    if (accepts !== eligible) throw Error(control.id + ': native eligibility disagrees ' + JSON.stringify(observed.diagnostics));
    profiles.push({id: control.id + (eligible ? '-eligible' : '-below'), files, options, accepts,
        diagnostics: observed.diagnostics, ...(accepts ? {declarations: observed.declarations} : {})});
}
const text = JSON.stringify({schema: 1, typescript: ts.version, cases, profile_controls: profiles, owner_controls: input.owner_controls}, null, 2) + '\n';
const file = path.join(directory, 'reference.json');
if (process.env.BLUEICE_WRITE_GT7_LIBRARY_MANIFEST === '1') fs.writeFileSync(file, text);
else if (fs.readFileSync(file, 'utf8').replaceAll('\r\n', '\n') !== text) throw Error('Frozen G-T7 native observations changed');
console.log('Verified ' + cases.length + ' native observations, ' + profiles.length + ' target/library controls and ' + input.owner_controls.length + ' separate owner controls');
