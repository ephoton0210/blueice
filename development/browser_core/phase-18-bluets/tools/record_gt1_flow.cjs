// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Native G-T1 diagnostics and declaration types; never derives expectations from BlueTS.
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const load = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
const ts = load(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/gt1_flow');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
if (input.schema !== 1 || input.typescript !== ts.version) throw new Error('Foreign G-T1 matrix');
const converted = ts.convertCompilerOptionsFromJson(input.options, root);
if (converted.errors.length) throw new Error('Invalid native compiler options');
const options = converted.options;
const libraries = new Map();
if (process.argv.includes('--outputs')) {
    const resolve = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
    const acorn = require(resolve(process.env.BLUEICE_ACORN_ORACLE, 'acorn', 'dist/acorn.js'));
    if (acorn.version !== '8.15.0') throw new Error('Unpinned Acorn');
    const reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json'), 'utf8'));
    const outputs = JSON.parse(fs.readFileSync(0, 'utf8'));
    const accepted = reference.cases.filter(row => row.accepts);
    if (outputs.length !== accepted.length) throw new Error('Incomplete emitted matrix');
    for (let index = 0; index < outputs.length; index++) {
        const row = outputs[index], native = accepted[index];
        if (row.id !== native.id) throw new Error(`Foreign declaration consumer: ${row.id}`);
        acorn.parse(row.javascript, {ecmaVersion:2020,sourceType:'module'});
        const base = path.join(root, '__gt1_consumer__', row.id);
        const consumer = path.join(base, 'consumer.ts');
        const sources = new Map([
            [path.join(base, 'blue.d.ts'), row.declaration],
            [path.join(base, 'native.d.ts'), native.declaration],
            [consumer, "type Equal<A,B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false; type Assert<T extends true> = T; type Forward = Assert<Equal<typeof import('./blue'), typeof import('./native')>>; type Reverse = Assert<Equal<typeof import('./native'), typeof import('./blue')>>;"],
        ]);
        const consumerOptions = {...options, moduleResolution:ts.ModuleResolutionKind.Node10};
        const host = ts.createCompilerHost(consumerOptions), read = host.getSourceFile;
        const fileExists = host.fileExists, readFile = host.readFile;
        const directoryExists = host.directoryExists;
        host.directoryExists = file => file === base || directoryExists(file);
        host.fileExists = file => sources.has(file) || fileExists(file);
        host.readFile = file => sources.get(file) ?? readFile(file);
        host.getSourceFile = (file, version, ...args) => {
            if (sources.has(file)) return ts.createSourceFile(file, sources.get(file), version, true);
            if (!libraries.has(file)) libraries.set(file, read(file, version, ...args));
            return libraries.get(file);
        };
        const diagnostics = ts.getPreEmitDiagnostics(ts.createProgram([consumer], consumerOptions, host));
        if (diagnostics.length) throw new Error(`${row.id}: consumer ${ts.formatDiagnostics(diagnostics, host)}`);
        const source = input.cases.find(item => item.id === row.id);
        if (source.invoke) {
            const run = spawnSync(process.execPath, ['--input-type=module'], {input:row.javascript+'\n'+source.invoke,encoding:'utf8'});
            const actual = {exit:run.status,stdout:run.stdout.replaceAll('\r\n','\n'),stderr:run.stderr.replaceAll('\r\n','\n')};
            if (run.error || JSON.stringify(actual) !== JSON.stringify(native.runtime)) throw new Error(`${row.id}: execution mismatch`);
        }
    }
    process.stdout.write(`Verified ${outputs.length} native declaration consumers and Acorn outputs\n`);
    process.exit(0);
}
const cases = input.cases.map(row => {
    const name = path.join(root, '__gt1_flow__', row.id, 'entry.ts');
    const host = ts.createCompilerHost(options);
    const read = host.getSourceFile;
    host.getSourceFile = (file, version, ...args) => {
        if (file === name) return ts.createSourceFile(file, row.source, version, true);
        if (!libraries.has(file)) libraries.set(file, read(file, version, ...args));
        return libraries.get(file);
    };
    let declaration, javascript;
    host.writeFile = (file, text) => {
        if (file.endsWith('.d.ts')) declaration = text;
        if (file.endsWith('.js')) javascript = text;
    };
    const program = ts.createProgram([name], options, host);
    const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program));
    const describe = d => ({code:d.code, message:ts.flattenDiagnosticMessageText(d.messageText,'\n'),
        module: d.file ? (d.file.fileName === name ? 'entry.ts' : path.basename(d.file.fileName)) : null,
        start:d.start ?? null, length:d.length ?? null, related:(d.relatedInformation || []).map(describe)});
    const result = {id:row.id, accepts:diagnostics.length === 0, diagnostics:diagnostics.map(describe)};
    if (!diagnostics.length) {
        program.emit();
        if (typeof declaration !== 'string') throw new Error(`${row.id}: missing native declaration`);
        result.declaration = declaration.replaceAll('\r\n','\n');
        if (row.invoke) {
            const run = spawnSync(process.execPath, ['--input-type=module'], {input:javascript+'\n'+row.invoke,encoding:'utf8'});
            if (run.error || run.status !== 0) throw new Error(`${row.id}: native execution failed`);
            result.runtime = {exit:run.status,stdout:run.stdout.replaceAll('\r\n','\n'),stderr:run.stderr.replaceAll('\r\n','\n')};
        }
    }
    return result;
});
const text = JSON.stringify({schema:1,typescript:ts.version,options:input.options,cases}, null, 2) + '\n';
const output = path.join(directory, 'reference.json');
if (process.env.BLUEICE_WRITE_GT1_FLOW === '1') fs.writeFileSync(output, text);
else if (fs.readFileSync(output,'utf8').replaceAll('\r\n','\n') !== text) throw new Error('G-T1 native oracle changed');
process.stdout.write(`Verified ${cases.length} TypeScript ${ts.version} G-T1 diagnostic/declaration observations\n`);
