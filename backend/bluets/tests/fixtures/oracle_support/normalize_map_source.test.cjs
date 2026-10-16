// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const test = require('node:test');
const normalizeMapSource = require('./normalize_map_source.cjs');

test('normalizes native Windows file URLs to the platform-neutral corpus', () => {
    const directory = 'C:\\Users\\runner\\Temp\\root-maproot-7';
    const source = 'file:///C:/Users/runner/Temp/root-maproot-7/main.ts';
    assert.equal(normalizeMapSource(source, directory, '/roots'), '..<case>/main.ts');
    assert.equal(normalizeMapSource(source, `\\\\?\\${directory}`, '/roots'),
        '..<case>/main.ts');
    assert.equal(normalizeMapSource(source, directory, 'https://example.invalid/roots'),
        'file://<case>/main.ts');
});

test('retains already normalized relative sources', () => {
    assert.equal(normalizeMapSource('../main.ts', '/tmp/root-maproot-7', '/roots'), '../main.ts');
});
