// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// G-T4 requirements come only from the pinned native compiler, never BlueTS.
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const support = path.join(root, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/gt4_overload_chains');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
if (input.schema !== 1 || input.typescript !== ts.version) throw Error('Foreign G-T4 input');
const libraries = new Map(), base = path.join(root, '__gt4_overload_chains__');
function compile(source, sources = new Map(), options = input.options) {
    const converted = ts.convertCompilerOptionsFromJson(options, root);
    if (converted.errors.length) throw Error('Invalid native options');
    const native = converted.options, name = path.join(base, 'entry.ts');
    sources = new Map(sources); sources.set(name, source);
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
    let declaration, javascript;
    host.writeFile = (file, text) => {
        if (path.basename(file) === 'entry.d.ts') declaration = text.replaceAll('\r\n','\n');
        if (path.basename(file) === 'entry.js') javascript = text;
    };
    const program = ts.createProgram([name], native, host);
    const describe = d => ({code:d.code,message:ts.flattenDiagnosticMessageText(d.messageText,'\n'),module:d.file?path.basename(d.file.fileName):null,start:d.start??null,length:d.length??null,related:(d.relatedInformation||[]).map(describe)});
    const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program)).map(describe);
    if (!diagnostics.length) program.emit();
    return {diagnostics,declaration,javascript};
}
function source(form, verdict) {
    return `// UTF-16 control: 😀\n${form.prefix}\n${form[verdict]}\n`;
}
function execute(javascript, invoke) {
    const run = spawnSync(process.execPath, ['--input-type=module'], {input:javascript+'\n'+invoke,encoding:'utf8'});
    if (run.error) throw run.error;
    return {exit:run.status,stdout:run.stdout.replaceAll('\r\n','\n'),stderr:run.stderr.replaceAll('\r\n','\n')};
}
if (process.argv.includes('--outputs')) {
    const reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json'), 'utf8'));
    const accepted = reference.cases.filter(row => row.accepts), outputs = JSON.parse(fs.readFileSync(0, 'utf8'));
    const resolve = require(path.join(support, 'resolve_package.cjs'));
    const acorn = require(resolve(process.env.BLUEICE_ACORN_ORACLE, 'acorn', 'dist/acorn.js'));
    if (acorn.version !== '8.15.0' || outputs.length !== accepted.length) throw Error('Foreign/incomplete outputs');
    for (let index = 0; index < outputs.length; index++) {
        const actual = outputs[index], expected = accepted[index];
        if (actual.id !== expected.id || actual.declaration !== expected.declaration) throw Error('Non-exact G-T4 declaration');
        acorn.parse(actual.javascript,{ecmaVersion:2022,sourceType:'module'});
        // Exact bytes were checked above. Separate modules allocate distinct
        // unique symbols even for identical text; align only those identities
        // for the bidirectional consumer, not declarations or diagnostics.
        const native = expected.declaration.replace(/export declare const (\w+): unique symbol;/g,
            (_, name) => `export declare const ${name}: typeof import('./blue').${name};`);
        const sources = new Map([[path.join(base,'blue.d.ts'),actual.declaration],[path.join(base,'native.d.ts'),native]]);
        const consumer = "type Equal<A,B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false; type Assert<T extends true> = T; type Forward = Assert<Equal<typeof import('./blue'), typeof import('./native')>>; type Reverse = Assert<Equal<typeof import('./native'), typeof import('./blue')>>;";
        const {diagnostics} = compile(consumer, sources, expected.options);
        if (diagnostics.length) throw Error(`${actual.id}: native consumer ${JSON.stringify(diagnostics)}`);
        if (expected.consumer) for (const declaration of [actual.declaration, expected.declaration]) {
            const {diagnostics} = compile(expected.consumer, new Map([[path.join(base,'candidate.d.ts'),declaration]]), expected.options);
            if (diagnostics.length) throw Error(`${actual.id}: exported-type consumer ${JSON.stringify(diagnostics)}`);
        }
        if (expected.invoke && JSON.stringify(execute(actual.javascript,expected.invoke)) !== JSON.stringify(expected.runtime)) throw Error(`${actual.id}: runtime mismatch`);
    }
    console.log(`Verified ${outputs.length} exact G-T4 consumers and syntax outputs`);
    process.exit(0);
}
const cases = [];
for (const form of input.forms) for (const verdict of ['accept','reject']) {
    const options = {...input.options, ...form.options};
    const text = source(form, verdict), observed = compile(text, new Map(), options), accepts = observed.diagnostics.length === 0;
    if (accepts !== (verdict === 'accept')) throw Error(`${form.id}-${verdict}: unexpected native verdict ${JSON.stringify(observed.diagnostics)}`);
    const row = {id:form.id+'-'+verdict,area:form.area,source:text,options,accepts,diagnostics:observed.diagnostics};
    if (accepts) {
        if (typeof observed.declaration !== 'string') throw Error('Missing native declaration');
        row.declaration = observed.declaration;
        if (form.consumer) {
            row.consumer = form.consumer;
            const {diagnostics} = compile(form.consumer, new Map([[path.join(base,'candidate.d.ts'),observed.declaration]]), options);
            if (diagnostics.length) throw Error(`${row.id}: native exported-type consumer failed ${JSON.stringify(diagnostics)}`);
        }
        if (form.invoke) {
            row.invoke = form.invoke; row.runtime = execute(observed.javascript,form.invoke);
            if (row.runtime.exit !== 0) throw Error('Native runtime failed');
        }
    }
    cases.push(row);
}
for (const control of input.budget_controls) {
    if (compile(control.source).diagnostics.length) throw Error(`${control.id}: native default-budget source rejected`);
}
for (const control of input.readonly_controls) for (const key of ['source','mutable']) {
    if (compile(control[key]).diagnostics.length) throw Error(`${control.id}/${key}: native policy control rejected`);
}
const text = JSON.stringify({schema:1,typescript:ts.version,cases,budget_controls:input.budget_controls,readonly_controls:input.readonly_controls},null,2)+'\n';
const file = path.join(directory,'reference.json');
if (process.env.BLUEICE_WRITE_GT4_OVERLOAD_CHAINS === '1') fs.writeFileSync(file,text);
else if (fs.readFileSync(file,'utf8').replaceAll('\r\n','\n') !== text) throw Error('G-T4 native observations changed');
console.log(`Verified ${cases.length} native G-T4 observations, ${input.budget_controls.length} budget controls and ${input.readonly_controls.length} readonly policy controls`);
