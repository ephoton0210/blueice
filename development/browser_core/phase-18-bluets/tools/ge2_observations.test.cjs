// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const ts = require(path.resolve(__dirname, '../../../../backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const { inspect } = require('./ge2_observations.cjs');

test('Node createRequire aliases retain dependencies despite source-name collisions', () => {
  for (const builtin of ['module', 'node:module']) {
    const outputs = { 'main.mjs': `import {createRequire as create_2} from "${builtin}"; const require_2=create_2(import.meta.url); const __require=1; const value=require_2("./dep.cjs");` };
    assert.deepEqual(inspect(ts, outputs, new Set()), { 'main.mjs': { dependencies: ['./dep.cjs', builtin], helperImports: [] } });
  }
});
test('AMD helper inventory retains the imported ABI independently of namespace spelling', () => {
  const outputs = { 'main.js': 'define(["require","exports","tslib"],function(r,e,library){e.result=library.__awaiter(this,[],void 0,function*(){return 42;});});' };
  assert.deepEqual(inspect(ts, outputs, new Set(['__awaiter'])), { 'main.js': { dependencies: ['tslib'], helperImports: ['__awaiter'] } });
});
test('System dependencies and imported ABI remain attached to their own artifact', () => {
  const outputs = { 'main.js': 'System.register(["./dep.js","tslib"],function(e){return {setters:[],execute:function(){return library.__generator(this,function(){return [2];});}};});', 'dep.js': 'export const value=42;' };
  assert.deepEqual(inspect(ts, outputs, new Set(['__generator'])), { 'dep.js': { dependencies: [], helperImports: [] }, 'main.js': { dependencies: ['./dep.js','tslib'], helperImports: ['__generator'] } });
});
