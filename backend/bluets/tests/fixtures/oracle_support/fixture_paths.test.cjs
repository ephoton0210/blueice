// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const path = require('node:path');
const test = require('node:test');
const relativeFixturePath = require('./fixture_paths.cjs');

test('keeps files inside the same fixture and excludes unrelated library files', () => {
    const directory = path.join('/tmp', 'bluets-oracle-123', 'case-a');
    assert.equal(relativeFixturePath(directory, path.join(directory, 'node_modules/pkg/index.d.ts')),
        'node_modules/pkg/index.d.ts');
    assert.equal(relativeFixturePath(directory, '/opt/typescript/lib/lib.es2022.d.ts'), null);
});

test('recovers a fixture path when Windows short and long parent names differ', () => {
    const directory = '/tmp/RUNNER~1/Temp/bluets-oracle-123/case-a';
    const filename = '/tmp/runneradmin/Temp/bluets-oracle-123/case-a/node_modules/pkg/index.d.ts';
    assert.equal(relativeFixturePath(directory, filename), 'node_modules/pkg/index.d.ts');
});

test('Windows native paths recover 8.3 aliases and exclude another drive',
    { skip: process.platform !== 'win32' }, () => {
        const directory = 'C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\bluets-oracle-123\\case-a';
        const source = 'C:/Users/runneradmin/AppData/Local/Temp/bluets-oracle-123/case-a/node_modules/pkg/index.d.ts';
        assert.equal(relativeFixturePath(directory, source), 'node_modules/pkg/index.d.ts');
        assert.equal(relativeFixturePath(directory,
            'D:/a/_temp/bluets-oracles/node_modules/typescript/lib/lib.es2022.d.ts'), null);
    });
