// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const { oracleCommands, appendEnvironment } = require('./select_oracle_commands.cjs');

test('selects explicit npm shims on Windows and Unix', () => {
    const root = path.join('temporary root', 'bluets-oracles');
    assert.deepEqual(oracleCommands(root, 'win32'), {
        typescript: path.join(root, 'node_modules', '.bin', 'tsc.cmd'),
        acorn: path.join(root, 'node_modules', '.bin', 'acorn.cmd'),
    });
    assert.deepEqual(oracleCommands(root, 'darwin'), {
        typescript: path.join(root, 'node_modules', '.bin', 'tsc'),
        acorn: path.join(root, 'node_modules', '.bin', 'acorn'),
    });
});

test('exports both absolute commands through the Actions environment file', () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-oracle-env-'));
    const file = path.join(directory, 'github-env');
    try {
        const commands = oracleCommands(path.join(directory, 'installed oracles'), 'win32');
        appendEnvironment(file, commands);
        assert.equal(fs.readFileSync(file, 'utf8'),
            `BLUEICE_BLUETSC_ORACLE=${commands.typescript}\n` +
            `BLUEICE_ACORN_ORACLE=${commands.acorn}\n`);
    } finally {
        fs.rmSync(directory, { recursive: true, force: true });
    }
});
