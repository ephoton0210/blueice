// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Observe the pinned compiler's ES2022 numeric-literal cache initialization.
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const Module = require('module');
const root = path.resolve(__dirname, '../../../..');
const fixtures = path.join(root, 'backend/bluets/tests/fixtures');
const load = require(path.join(fixtures, 'oracle_support/load_typescript.cjs'));
const original = load(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
if (original.version !== '5.9.3') throw new Error('Expected TypeScript 5.9.3');
const options = {target:original.ScriptTarget.ES2022, module:original.ModuleKind.ES2022, strict:true, noEmit:true};
const filename = path.join(path.dirname(original.getDefaultLibFilePath(options)), 'typescript.js');
const source = fs.readFileSync(filename, 'utf8');
const anchor = '    return numberLiteralTypes.get(value) || (numberLiteralTypes.set(value, type = createLiteralType(256 /* NumberLiteral */, value)), type);';
if (source.split(anchor).length !== 2) throw new Error('Pinned numeric cache implementation changed');
const literals = [];
const observe = value => literals.push(value);
process.on('blueice-numeric-literal', observe);
const instrumented = new Module(filename, module);
instrumented.filename = filename;
instrumented.paths = module.paths;
instrumented._compile(source.replace(anchor,
    "    if (!numberLiteralTypes.has(value)) process.emit('blueice-numeric-literal', value);\n" + anchor), filename);
const ts = instrumented.exports;
const virtual = ts.normalizePath(path.join(fixtures, 'oracle_support', 'operator_literal_cache.ts'));
const host = ts.createCompilerHost(options);
const getSourceFile = host.getSourceFile;
host.getSourceFile = (file, version, onError, createNew) => file === virtual
    ? ts.createSourceFile(file, 'export {};', version, true)
    : getSourceFile(file, version, onError, createNew);
const diagnostics = ts.getPreEmitDiagnostics(ts.createProgram([virtual], options, host));
process.removeListener('blueice-numeric-literal', observe);
if (diagnostics.length) throw new Error(ts.formatDiagnostics(diagnostics, host));
const record = JSON.stringify({version:ts.version, target:'ES2022',
    compiler_sha256:crypto.createHash('sha256').update(source).digest('hex'), literals}, null, 2) + '\n';
const destination = path.join(fixtures, 'typescript_oracle', 'operator-literal-order-reference.json');
if (process.env.BLUEICE_WRITE_OPERATOR_LITERAL_ORDER === '1') fs.writeFileSync(destination, record);
else if (fs.readFileSync(destination, 'utf8').replaceAll('\r\n', '\n') !== record)
    throw new Error('Pinned numeric literal initialization order differs');
process.stdout.write(JSON.stringify({version:ts.version, literals:literals.length}) + '\n');
