// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// G-C1 positive/negative controls and three actual native emitted executions.
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const load = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
const ts = load(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const directory = path.join(root, 'backend/bluets/tests/fixtures/gc1_source_controls');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
if (input.schema !== 1 || input.typescript !== ts.version) throw new Error('Foreign source controls');
const options = {target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext, strict: true};
const libraryCache = new Map();
const cases = input.cases.map(row => {
    const name = path.join(root, '__gc1_sources__', row.id, 'entry.ts');
    const host = ts.createCompilerHost(options);
    const read = host.getSourceFile;
    host.getSourceFile = (file, version, ...args) => {
        if (file === name) return ts.createSourceFile(file, row.source, version, true);
        if (!libraryCache.has(file)) libraryCache.set(file, read(file, version, ...args));
        return libraryCache.get(file);
    };
    let javascript;
    host.writeFile = (file, text) => { if (file.endsWith('.js')) javascript = text; };
    const program = ts.createProgram([name], options, host);
    const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program));
    const result = {id: row.id, accepts: diagnostics.length === 0, diagnostics: diagnostics.map(d => ({code:d.code, message:ts.flattenDiagnosticMessageText(d.messageText,'\n'),start:d.start,length:d.length}))};
    if (row.runtime) {
        if (diagnostics.length) throw new Error(`${row.id}: runtime fixture rejected`);
        program.emit();
        if (typeof javascript !== 'string') throw new Error('Missing native JavaScript');
        const run = spawnSync(process.execPath, ['--input-type=module'], {input:javascript,encoding:'utf8'});
        if (run.error || run.status !== 0) throw new Error(`${row.id}: ${run.error || run.stderr}`);
        result.runtime = {exit:run.status,stdout:run.stdout.replaceAll('\r\n','\n'),stderr:run.stderr.replaceAll('\r\n','\n')};
    }
    return result;
});
const text = JSON.stringify({schema:1,typescript:ts.version,cases}, null, 2) + '\n';
const output = path.join(directory, 'reference.json');
if (process.env.BLUEICE_WRITE_GC1_SOURCE_MATRIX === '1') fs.writeFileSync(output, text);
else if (fs.readFileSync(output,'utf8').replaceAll('\r\n','\n') !== text) throw new Error('G-C1 source oracle changed');
process.stdout.write(`Verified ${cases.length} TypeScript ${ts.version} source controls and three native executions\n`);
