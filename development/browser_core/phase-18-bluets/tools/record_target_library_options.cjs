// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs');
const os = require('os');
const path = require('path');
const root = path.resolve(__dirname, '../../../..');
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const reference = JSON.parse(fs.readFileSync(path.join(root, 'backend/bluets/tests/fixtures/target_library_options/reference.json'), 'utf8'));
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-library-reference-'));
try {
    const cases = reference.cases.map(({accepts, codes, ...entry}) => {
        const directory = path.join(work, entry.id);
        fs.mkdirSync(directory);
        const file = path.join(directory, 'main.ts');
        fs.writeFileSync(file, entry.source);
        const parsed = ts.convertCompilerOptionsFromJson(entry.options, directory);
        if (parsed.errors.length) throw new Error(`Invalid options: ${entry.id}`);
        const program = ts.createProgram([file], {...parsed.options, noEmit: true, skipLibCheck: true});
        const diagnostics = ts.getPreEmitDiagnostics(program);
        return {...entry, accepts: diagnostics.length === 0, codes: diagnostics.map(d => d.code)};
    });
    process.stdout.write(JSON.stringify({typescriptVersion: ts.version, cases}));
} finally {
    fs.rmSync(work, {recursive: true, force: true});
}
