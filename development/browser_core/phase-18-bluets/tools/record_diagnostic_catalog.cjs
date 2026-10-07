// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Pinned diagnostic metadata, without copying TypeScript checking algorithms.
const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '../../../..');
const load = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
const ts = load(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
if (ts.version !== '5.9.3') throw new Error(`Expected TypeScript 5.9.3, received ${ts.version}`);
const templates = Object.fromEntries(Object.values(ts.Diagnostics)
    .sort((left, right) => left.code - right.code)
    .map(item => [item.code, item.message]));
const filename = path.join(root, '__diagnostic_catalog__.ts');
const options = { target: ts.ScriptTarget.ES2022, lib: ['lib.es2022.d.ts'], strict: true, noEmit: true };
const host = ts.createCompilerHost(options);
const read = host.getSourceFile;
host.getSourceFile = (name, version, onError, createNew) => name === filename
    ? ts.createSourceFile(name, '', version)
    : read(name, version, onError, createNew);
const program = ts.createProgram([filename], options, host);
const checker = program.getTypeChecker();
const source = program.getSourceFile(filename);
const constructors = {};
const members = {};
for (const name of ['Object', 'Array', 'String', 'Number', 'Boolean', 'Date', 'RegExp',
    'Error', 'EvalError', 'RangeError', 'ReferenceError', 'SyntaxError', 'TypeError', 'URIError', 'JSON', 'Promise']) {
    const symbol = checker.resolveName(name, source, ts.SymbolFlags.Value, false);
    if (!symbol) throw new Error(`Missing library value ${name}`);
    const value = checker.getTypeOfSymbolAtLocation(symbol, source);
    constructors[name] = value.getConstructSignatures().length;
    members[`${name}Constructor`] = Object.fromEntries(value.getProperties().map(member => [
        member.name, checker.getTypeOfSymbolAtLocation(member, source).getCallSignatures().length,
    ]).sort(([left], [right]) => left.localeCompare(right, 'en')));
}
for (const name of ['Array', 'String', 'Number', 'Boolean']) {
    const symbol = checker.resolveName(name, source, ts.SymbolFlags.Type, false);
    const value = checker.getDeclaredTypeOfSymbol(symbol);
    members[name] = Object.fromEntries(value.getProperties().map(member => [
        member.name, checker.getTypeOfSymbolAtLocation(member, source).getCallSignatures().length,
    ]).sort(([left], [right]) => left.localeCompare(right, 'en')));
}
const catalog = { version: ts.version, templates, constructors, members };
const text = JSON.stringify(catalog, null, 2) + '\n';
const destination = path.join(root, 'backend/bluets/src/diagnostic/typescript-5.9.3.json');
if (process.env.BLUEICE_WRITE_DIAGNOSTICS_MATRIX === '1') fs.writeFileSync(destination, text);
else if (fs.readFileSync(destination, 'utf8') !== text) throw new Error('Pinned diagnostic catalog changed');
process.stdout.write(`Recorded ${Object.keys(templates).length} templates and ${Object.keys(members).length} library member catalogs\n`);
