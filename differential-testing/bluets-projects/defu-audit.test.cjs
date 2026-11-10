// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const {test} = require('node:test');
const {assess, applyEdit, verifyReference} = require('./defu-audit.cjs');
const {unpackFixture} = require('./audit.cjs');
const manifest = require('./defu-v6.1.4.json');

function successful() {
  return {cold: {exit: 0}, runtime: {passed: manifest.runtimeScenarios},
    consumers: {positive: true, negative: true, typeBefore: true},
    incremental: {passed: true}, typeAfterRejected: true};
}
test('Native baseline changes cannot silently replace frozen declarations or edit observations', () => {
  const reference = require('./defu-v6.1.4.native.json');
  assert.doesNotThrow(() => verifyReference(manifest, reference, reference.nativeAcceptance,
    reference.declarations, reference.typescript));
  const altered = JSON.parse(JSON.stringify(reference.declarations));
  altered['types.d.ts'].text += '\n';
  assert.throws(() => verifyReference(manifest, reference, reference.nativeAcceptance,
    altered, reference.typescript), /declaration/);
  const native = JSON.parse(JSON.stringify(reference.nativeAcceptance));
  native.stages[1].artifacts['unexpected.js'] = 'foreign';
  assert.throws(() => verifyReference(manifest, reference, native,
    reference.declarations, reference.typescript), /edit observations/);
});
test('a refused multi-file project cannot inherit Native downstream successes', () => {
  assert.equal(assess(successful(), {cold: {exit: 1}}, {equal: true}, manifest), false);
});
test('all runtime scenarios, consumers and actual incremental stages are required', () => {
  assert.equal(assess(successful(), successful(), {equal: true}, manifest), true);
  for (const field of ['runtime', 'consumers', 'incremental', 'typeAfterRejected']) {
    const value = successful();
    delete value[field];
    assert.equal(assess(successful(), value, {equal: true}, manifest), false, field);
  }
  const value = successful();
  value.runtime.passed = manifest.runtimeScenarios.slice(1);
  assert.equal(assess(successful(), value, {equal: true}, manifest), false);
  value.runtime.passed = Array(12).fill(manifest.runtimeScenarios[0]);
  assert.equal(assess(successful(), value, {equal: true}, manifest), false);
  assert.equal(assess(successful(), successful(), {equal: false}, manifest), false);
});
test('an exported type edit must change the dependent declaration-consumer verdict', () => {
  for (const typeBefore of [false, undefined]) {
    const value = successful(); value.consumers.typeBefore = typeBefore;
    assert.equal(assess(successful(), value, {equal: true}, manifest), false);
  }
  const value = successful(); value.typeAfterRejected = false;
  assert.equal(assess(successful(), value, {equal: true}, manifest), false);
});
test('authored edits require the frozen exact occurrence and cannot escape the project', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'blueice-defu-edit-'));
  try {
    fs.writeFileSync(path.join(root, 'input.ts'), 'export const value = true;\n');
    applyEdit(root, {path: 'input.ts', from: 'true', to: 'Boolean(true)'});
    assert.equal(fs.readFileSync(path.join(root, 'input.ts'), 'utf8'), 'export const value = Boolean(true);\n');
    assert.throws(() => applyEdit(root, {path: 'input.ts', from: 'missing', to: 'false'}));
    assert.throws(() => applyEdit(root, {path: '../outside.ts', from: 'x', to: 'y'}));
    fs.writeFileSync(path.join(root, 'input.ts'), 'true true');
    assert.throws(() => applyEdit(root, {path: 'input.ts', from: 'true', to: 'false'}));
  } finally { fs.rmSync(root, {recursive: true, force: true}); }
});
test('the fixture retains every upstream production source and its original notices', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'blueice-defu-fixture-'));
  try {
    unpackFixture(root, manifest);
    assert.deepEqual(manifest.productionSources, ['src/_utils.ts', 'src/defu.ts', 'src/types.ts']);
    assert(fs.readFileSync(path.join(root, 'src/_utils.ts'), 'utf8').includes('Copyright (c) Sindre Sorhus'));
    assert(fs.readFileSync(path.join(root, 'LICENSE'), 'utf8').includes('MIT'));
    assert.equal(JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')).version, manifest.packageVersion);
    assert.equal(manifest.upstreamTag, 'v6.1.4');
    assert.equal(manifest.packageVersion, '6.1.3', 'preserve the actual upstream tag metadata');
  } finally { fs.rmSync(root, {recursive: true, force: true}); }
});
