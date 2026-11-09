// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const {test} = require('node:test');
const {declarations, compatible, unpackFixture} = require('./audit.cjs');

test('a refused build cannot become compatible through missing downstream results', () => {
  assert.equal(compatible({cold: {exit: 0}}, {cold: {exit: 1}}, {equal: true}), false);
});

test('declarations compare complete inventories and exact bytes', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'blueice-project-declarations-'));
  try {
    const a = path.join(root, 'a'), b = path.join(root, 'b');
    fs.mkdirSync(a); fs.mkdirSync(b);
    assert.equal(declarations(a, b).equal, false, 'two empty outputs are not agreement');
    fs.writeFileSync(path.join(a, 'index.d.ts'), 'export declare const value: number;\n');
    fs.copyFileSync(path.join(a, 'index.d.ts'), path.join(b, 'index.d.ts'));
    assert.equal(declarations(a, b).equal, true);
    fs.writeFileSync(path.join(b, 'index.d.ts'), 'export declare const value: number;\r\n');
    assert.equal(declarations(a, b).equal, false, 'newline bytes matter');
    fs.copyFileSync(path.join(a, 'index.d.ts'), path.join(b, 'index.d.ts'));
    fs.writeFileSync(path.join(b, 'extra.d.ts'), 'export {};\n');
    assert.equal(declarations(a, b).equal, false, 'extra files matter');
  } finally {
    fs.rmSync(root, {recursive: true, force: true});
  }
});

test('the pinned fixture preserves upstream source and license bytes', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'blueice-project-fixture-'));
  try {
    const manifest = require('./mitt-3.0.1.json');
    unpackFixture(root, manifest);
    assert(fs.readFileSync(path.join(root, 'LICENSE'), 'utf8').includes('Copyright (c) 2021 Jason Miller'));
    assert.equal(fs.readFileSync(path.join(root, 'src/index.ts'), 'utf8').split('\n').length, 124);
    assert.throws(() => unpackFixture(root, {...manifest, archiveSha256: '0'.repeat(64)}), /hash/);
  } finally {
    fs.rmSync(root, {recursive: true, force: true});
  }
});
