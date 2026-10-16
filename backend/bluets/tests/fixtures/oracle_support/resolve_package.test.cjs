// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const loadTypeScript = require('./load_typescript.cjs');
const loadAcorn = require('./load_acorn.cjs');

test('npm Windows command shims resolve adjacent pinned packages', () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'blue ts oracle '));
    const modules = path.join(directory, 'node_modules');
    const binaries = path.join(modules, '.bin');
    const typescript = path.join(modules, 'typescript');
    const acorn = path.join(modules, 'acorn');
    try {
        fs.mkdirSync(binaries, { recursive: true });
        fs.mkdirSync(path.join(typescript, 'lib'), { recursive: true });
        fs.mkdirSync(path.join(typescript, 'bin'), { recursive: true });
        fs.mkdirSync(path.join(acorn, 'dist'), { recursive: true });
        const tsc = path.join(binaries, 'tsc.cmd');
        const acornCommand = path.join(binaries, 'acorn.cmd');
        fs.writeFileSync(tsc, '@echo off\r\n');
        fs.writeFileSync(acornCommand, '@echo off\r\n');
        fs.chmodSync(tsc, 0o755);
        fs.chmodSync(acornCommand, 0o755);
        fs.writeFileSync(path.join(typescript, 'lib/typescript.js'),
            'module.exports = { version: "5.9.3" };\n');
        fs.writeFileSync(path.join(typescript, 'bin/tsc'), '');
        fs.writeFileSync(path.join(acorn, 'dist/acorn.js'),
            'module.exports = { version: "8.15.0" };\n');
        assert.equal(loadTypeScript(tsc).version, '5.9.3');
        assert.equal(loadAcorn(acornCommand).version, '8.15.0');
        assert.equal(loadTypeScript.cliPath(tsc),
            fs.realpathSync(path.join(typescript, 'bin/tsc')));
    } finally {
        fs.rmSync(directory, { recursive: true, force: true });
    }
});
