// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Local experimental patch, not an upstream TypeScript release or stock oracle.
// The replacement fragment below derives from Microsoft/TypeScript v5.9.3,
// src/compiler/transformers/module/system.ts (createSystemModuleBody), as
// distributed in typescript@5.9.3/lib/typescript.js. Its upstream notice follows:
/*!
Copyright (c) Microsoft Corporation. All rights reserved.
Licensed under the Apache License, Version 2.0 (the "License"); you may not use
this file except in compliance with the License. You may obtain a copy of the
License at http://www.apache.org/licenses/LICENSE-2.0

THIS CODE IS PROVIDED ON AN *AS IS* BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
KIND, EITHER EXPRESS OR IMPLIED, INCLUDING WITHOUT LIMITATION ANY IMPLIED
WARRANTIES OR CONDITIONS OF TITLE, FITNESS FOR A PARTICULAR PURPOSE,
MERCHANTABLITY OR NON-INFRINGEMENT.

See the Apache Version 2.0 License for specific language governing permissions
and limitations under the License.
*/

const fs = require('node:fs'), path = require('node:path'), Module = require('node:module');
const assert = require('node:assert/strict'), { sha256 } = require('./ge5_inputs.cjs');
const stockSha256 = '3ae902c92cc44dace175c0e69e13a4b0899f6983c6121d76b9ab8dd5795e7675';
const patchId = 'system-execute-generated-brace-v1';
const original = `            factory2.createBlock(
              executeStatements,
              /*multiLine*/
              true
            )`;
const replacement = `            setEmitFlags(factory2.createBlock(
              executeStatements,
              /*multiLine*/
              true
            ), 768 /* NoTokenSourceMaps */)`;

function patchSource(source) {
  assert.equal(sha256(source), stockSha256, 'TypeScript 5.9.3 bundle hash changed; refusing patch');
  assert.equal(source.split(original).length, 2, 'expected exactly one execute block');
  const offset = source.indexOf(original);
  assert(offset > source.indexOf('  function createSystemModuleBody(node, dependencyGroups) {'));
  assert(offset < source.indexOf('  function addExportStarIfNeeded(statements) {'));
  // The wrapper has no source brace. Suppress only its token mappings, retaining
  // all mappings produced by nested user statements. Do not clamp coordinates.
  return source.replace(original, replacement);
}

function load(filename) {
  const instance = new Module(filename, module);
  instance.filename = filename; instance.paths = Module._nodeModulePaths(path.dirname(filename));
  instance._compile(patchSource(fs.readFileSync(filename, 'utf8')), filename);
  assert.equal(instance.exports.version, '5.9.3');
  // Deliberately leave require.cache and the installed stock package untouched.
  return instance.exports;
}
module.exports = { stockSha256, patchId, patchSource, load };
