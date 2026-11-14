// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const test = require('node:test');
const path = require('node:path');
const attach = require('./virtual_compiler_host.cjs');
const load = require('../../../../backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs');
const ts = load(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
assert.equal(ts.version, '5.9.3');

function compile(directory, entry, text, insensitive) {
    const options = {strict: true, target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Node10,
        declaration: true, noEmitOnError: true, newLine: ts.NewLineKind.LineFeed,
        lib: ['lib.es5.d.ts'], types: []};
    const files = new Map([[directory + '/types.d.ts', 'export declare const value: "ok";\n'],
        [entry, text]]);
    const host = ts.createCompilerHost(options);
    if (insensitive) {
        host.useCaseSensitiveFileNames = () => false;
        host.getCanonicalFileName = name => name.toLowerCase();
    }
    attach(ts, host, files);
    const outputs = {};
    host.writeFile = (file, data) => {outputs[path.basename(file)] = data;};
    const root = insensitive ? entry.replaceAll('\\', '/').toLowerCase() : entry;
    const program = ts.createProgram([root], options, host);
    const diagnostics = ts.getPreEmitDiagnostics(program);
    const emitted = program.emit();
    return {host, outputs, diagnostics: [...diagnostics, ...emitted.diagnostics]};
}

for (const [label, directory, entry, insensitive] of [
    ['POSIX', '/__blueice_virtual_oracle__', '/__blueice_virtual_oracle__/entry.ts', false],
    ['Windows separators', 'B:/__blueice_virtual_oracle__', 'B:\\__blueice_virtual_oracle__\\entry.ts', false],
    ['Windows casing', 'B:/__blueice_virtual_oracle__', 'B:\\__blueice_virtual_oracle__\\entry.ts', true],
]) {
    test(label + ': Native resolves imported declarations and emits exact bytes', () => {
        const result = compile(directory, entry,
            'import { value } from "./types";\nexport const result: "ok" = value;\n', insensitive);
        assert.deepEqual(result.diagnostics.map(row => [row.code, ts.flattenDiagnosticMessageText(row.messageText, '\n')]), []);
        assert.equal(result.outputs['entry.d.ts'], 'export declare const result: "ok";\n');
        assert.equal(result.host.fileExists(directory + '/missing.ts'), false);
        assert.equal(result.host.readFile(directory + '/missing.ts'), undefined);
    });
    test(label + ': Native still rejects a wrong exported type and emits nothing', () => {
        const result = compile(directory, entry,
            'import { value } from "./types";\nexport const result: number = value;\n', insensitive);
        assert.deepEqual([...new Set(result.diagnostics.map(row => row.code))], [2322]);
        assert.deepEqual(result.outputs, {});
    });
}
