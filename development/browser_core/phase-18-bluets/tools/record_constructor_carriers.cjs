// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Replay constructor capability controls without rewriting the pinned reference.
const assert = require('assert/strict');
const fs = require('fs');
const os = require('os');
const path = require('path');
const root = path.resolve(__dirname, '../../../..');
const fixtures = path.join(root, 'backend/bluets/tests/fixtures');
const ts = require(path.join(fixtures, 'oracle_support/load_typescript.cjs'))(
    process.env.BLUEICE_BLUETSC_ORACLE || 'tsc',
);
const reference = JSON.parse(fs.readFileSync(path.join(fixtures,
    'typescript_oracle/modifiers-constructor-carriers.json'), 'utf8'));
assert.equal(ts.version, reference.version);
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-constructor-carriers-'));
try {
    const entry = path.join(directory, 'main.ts');
    fs.writeFileSync(path.join(directory, 'base.ts'),
        'export class B { constructor(public value: number) {} }');
    for (const example of reference.cases) {
        fs.writeFileSync(entry, example.source);
        const program = ts.createProgram([entry], {
            strict: true, noEmit: true, allowImportingTsExtensions: true,
            target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022,
        });
        const diagnostics = ts.getPreEmitDiagnostics(program).map(item => ({
            code: item.code, message: ts.flattenDiagnosticMessageText(item.messageText, '\n'),
            start: item.start, length: item.length,
        }));
        assert.deepEqual(diagnostics, example.diagnostics, example.name);
    }
} finally {
    fs.rmSync(directory, {recursive: true, force: true});
}
console.log(`${reference.cases.length} constructor capability controls agree with TypeScript ${ts.version}`);
