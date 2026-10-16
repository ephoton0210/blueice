// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const test = require('node:test');
const redactRoot = require('./redact_root.cjs');

test('redacts a Windows root in either separator spelling', () => {
    const root = 'C:\\Users\\RUNNER~1\\Temp\\bluets-source-extras';
    assert.equal(redactRoot(`File '${root}\\value.js'`, root),
        "File '<root>/value.js'");
    assert.equal(redactRoot(
        "File 'C:/Users/RUNNER~1/Temp/bluets-source-extras/value.js'", root),
        "File '<root>/value.js'");
});

test('retains unrelated diagnostic text', () => {
    assert.equal(redactRoot('Unknown module ./value.js', '/tmp/project'),
        'Unknown module ./value.js');
});
