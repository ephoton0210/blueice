// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Reproduce source witnesses for diagnostic families beyond the language matrix.
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../../../..');
const load = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
const ts = load(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
if (ts.version !== '5.9.3') throw new Error(`Expected TypeScript 5.9.3, received ${ts.version}`);
const destination = path.join(root, 'backend/bluets/tests/fixtures/diagnostics/source-families.json');
const reference = JSON.parse(fs.readFileSync(destination, 'utf8'));
const libraries = new Map();
const cases = reference.cases.map(row => {
    const filename = path.join(root, '__diagnostic_witnesses__', row.id, 'main.ts');
    const options = { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022,
        strict: true, noEmit: true, noUnusedLocals: row.flags.includes('--noUnusedLocals') };
    const host = ts.createCompilerHost(options);
    const read = host.getSourceFile;
    host.getSourceFile = (name, version, error, fresh) => {
        if (name === filename) return ts.createSourceFile(name, row.source, version);
        if (!libraries.has(name)) libraries.set(name, read(name, version, error, fresh));
        return libraries.get(name);
    };
    const program = ts.createProgram([filename], options, host);
    const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program));
    return { id: row.id, source: row.source, blueMessage: row.blueMessage, flags: row.flags,
        accepts: !diagnostics.some(diagnostic => diagnostic.category === ts.DiagnosticCategory.Error),
        diagnostics: diagnostics.map(diagnostic => ({ code: diagnostic.code,
            message: ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'),
            start: diagnostic.start, length: diagnostic.length })) };
});
const text = JSON.stringify({ version: ts.version, cases }, null, 2) + '\n';
if (process.env.BLUEICE_WRITE_DIAGNOSTICS_MATRIX === '1') fs.writeFileSync(destination, text);
else if (fs.readFileSync(destination, 'utf8') !== text) throw new Error('Diagnostic source witnesses changed');
process.stdout.write(`Recorded ${cases.length} diagnostic source witnesses\n`);
