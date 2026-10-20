// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native G-T3 observations: expectations never depend on BlueTS output.
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const support = path.join(root, 'backend/bluets/tests/fixtures/oracle_support');
const ts = require(path.join(support, 'load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/gt3_operators');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
if (input.schema !== 1 || input.typescript !== ts.version) throw Error('Foreign G-T3 input');
const libraries = new Map();
const base = path.join(root, '__gt3_operators__');
function compile(source, options, sources = new Map()) {
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
        const key = `${native.target}:${file}`;
        if (!libraries.has(key)) libraries.set(key, read(file, version, ...args));
        return libraries.get(key);
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
function execute(javascript, invoke) {
    const run = spawnSync(process.execPath, ['--input-type=module'], {input:javascript+'\n'+invoke,encoding:'utf8'});
    if (run.error) throw run.error;
    return {exit:run.status,stdout:run.stdout.replaceAll('\r\n','\n'),stderr:run.stderr.replaceAll('\r\n','\n')};
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
        if (actual.id !== expected.id || actual.declaration !== expected.declaration) throw Error('Non-exact G-T3 declaration');
        acorn.parse(actual.javascript,{ecmaVersion:expected.options.target === 'ES2020' ? 2020 : 2022,sourceType:'module'});
        const sources = new Map([[path.join(base,'blue.d.ts'),actual.declaration],[path.join(base,'native.d.ts'),expected.declaration]]);
        const consumer = "type Equal<A,B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false; type Assert<T extends true> = T; type Forward = Assert<Equal<typeof import('./blue'), typeof import('./native')>>; type Reverse = Assert<Equal<typeof import('./native'), typeof import('./blue')>>;";
        const {diagnostics} = compile(consumer, expected.options, sources);
        if (diagnostics.length) throw Error(`${actual.id}: native consumer ${JSON.stringify(diagnostics)}`);
        if (expected.invoke && JSON.stringify(execute(actual.javascript,expected.invoke)) !== JSON.stringify(expected.runtime)) throw Error(`${actual.id}: runtime mismatch`);
    }
    console.log(`Verified ${outputs.length} exact G-T3 consumers and syntax outputs`);
    process.exit(0);
}
const cases = [];
for (const form of input.forms) for (const verdict of ['accept','reject']) {
    const source = `// UTF-16 control: 😀\n${form.prefix}\nexport const value: ${form.annotation} = ${form[verdict]};\n`;
    const options = {...input.options,...(form.target ? {target:form.target} : {})};
    const observed = compile(source, options);
    const accepts = observed.diagnostics.length === 0;
    if (accepts !== (verdict === 'accept')) throw Error(`${form.id}-${verdict}: unexpected native verdict ${JSON.stringify(observed.diagnostics)}`);
    const row = {id:form.id+'-'+verdict,area:form.area,source,options,accepts,diagnostics:observed.diagnostics};
    if (accepts) {
        if (typeof observed.declaration !== 'string') throw Error('Missing native declaration');
        row.declaration = observed.declaration;
        if (form.invoke) {
            row.invoke = form.invoke; row.runtime = execute(observed.javascript,form.invoke);
            if (row.runtime.exit !== 0) throw Error('Native runtime failed');
        }
    }
    cases.push(row);
}
for (const control of input.budget_controls) {
    const native = compile(control.source,input.options);
    if (native.diagnostics.length) throw Error(`${control.id}: native default budget source rejected`);
}
const text = JSON.stringify({schema:1,typescript:ts.version,cases,budget_controls:input.budget_controls},null,2)+'\n';
const file = path.join(directory,'reference.json');
if (process.env.BLUEICE_WRITE_GT3_OPERATORS === '1') fs.writeFileSync(file,text);
else if (fs.readFileSync(file,'utf8').replaceAll('\r\n','\n') !== text) throw Error('G-T3 native observations changed');
console.log(`Verified ${cases.length} native G-T3 observations and ${input.budget_controls.length} default-budget controls`);
