// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Verify both preserved Native matrices; never rewrite expected observations.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
let root = __dirname;
while (!fs.existsSync(path.join(root, 'backend/bluets/Cargo.toml'))) {
  const parent = path.dirname(root);
  assert.notEqual(parent, root, 'Run this fixture verifier inside a BlueIce checkout');
  root = parent;
}
const ts = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(
  process.env.BLUEICE_BLUETSC_ORACLE || 'tsc',
);
assert.equal(ts.version, '5.9.3');
const maps = require(path.join(root, 'development/browser_core/phase-18-bluets/tools/ge5_maps.cjs'));

function native(source, newLine) {
  const filename = '/project/main.ts';
  const options = {
    strict: true, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext,
    sourceMap: true, newLine: ts.NewLineKind[newLine], lib: ['lib.es2022.d.ts'], types: [],
  };
  const host = ts.createCompilerHost(options), read = host.getSourceFile.bind(host), outputs = {};
  host.getSourceFile = (name, language, ...rest) => name === filename
    ? ts.createSourceFile(name, source, language, true) : read(name, language, ...rest);
  host.writeFile = (name, text) => { outputs[name] = text; };
  const program = ts.createProgram([filename], options, host);
  assert.deepEqual(ts.getPreEmitDiagnostics(program).map(item => ({
    code: item.code, message: ts.flattenDiagnosticMessageText(item.messageText, '\n'),
  })), [], 'Complete pinned Native program diagnostics');
  program.emit();
  return outputs;
}

const parentPath = path.join(__dirname, 'exploratory.v1.json');
const parentBytes = fs.readFileSync(parentPath), parent = JSON.parse(parentBytes);
const direct = JSON.parse(fs.readFileSync(path.join(__dirname, 'direct.v1.json')));
assert.equal(direct.parentMatrixSha256, crypto.createHash('sha256').update(parentBytes).digest('hex'));
assert.equal(parent.cases.length, 24);
assert.equal(direct.cases.length, 12);
for (const row of direct.cases) {
  assert.equal(row.id.split('/')[1], '.');
  assert.deepEqual(row, parent.cases.find(candidate => candidate.id === row.id), 'Unchanged parent observation');
}
for (const matrix of [parent, direct]) {
  assert.equal(matrix.typescript, ts.version);
  assert.equal(matrix.nativeRepeatPrograms, 2);
  for (const row of matrix.cases) {
    const first = native(row.source, row.newLine);
    assert.deepEqual(native(row.source, row.newLine), first, 'Repeat complete Native emission');
    const javascript = first['/project/main.js'];
    const body = javascript.replace(/(?:\r?\n)?\/\/# sourceMappingURL=[^\r\n]*(?:\r?\n)?$/, '');
    assert.equal(body, row.javascript, row.id + ': complete Native JavaScript');
    assert.deepEqual(maps.decode(first['/project/main.js.map'], javascript).segments, row.segments,
      row.id + ': every Native map segment');
  }
}
console.log(JSON.stringify({typescript: ts.version, preservedExploratoryInputs: 24, directInputs: 12,
  repeatNativePrograms: 2, originalInventoryCredit: 0, expectedObservationsChanged: false}));
