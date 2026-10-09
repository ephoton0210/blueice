// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs');
const path = require('path');
const ts = require('./load_typescript.cjs')(process.argv[2]);
const input = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
const converted = ts.convertCompilerOptionsFromJson(input.options, process.cwd());
const cases = input.cases.map(row => {
    const file = path.join(process.cwd(), 'function-context-control.ts');
    const options = converted.options;
    const host = ts.createCompilerHost(options);
    const original = host.getSourceFile.bind(host);
    host.getSourceFile = (name, ...args) => name === file
        ? ts.createSourceFile(file, row.source, options.target, true)
        : original(name, ...args);
    const program = ts.createProgram([file], options, host);
    const diagnostics = [...converted.errors, ...ts.getPreEmitDiagnostics(program)].map(d => ({
        code: d.code,
        start: d.start,
        length: d.length,
        message: ts.flattenDiagnosticMessageText(d.messageText, ' ')
    }));
    if ((diagnostics.length === 0) !== row.accepts) {
        throw new Error(JSON.stringify({ row, diagnostics }));
    }
    return { ...row, diagnostics };
});
process.stdout.write(JSON.stringify({ version: ts.version, options: input.options, cases }));
