// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('node:fs');
const path = require('node:path');
const repository = path.resolve(__dirname, '../../../..');
const loadTypeScript = require(path.join(repository,
    'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
const loadAcorn = require(path.join(repository,
    'backend/bluets/tests/fixtures/oracle_support/load_acorn.cjs'));

function oracleCommands(installRoot, platform = process.platform) {
    const suffix = platform === 'win32' ? '.cmd' : '';
    const bin = path.join(installRoot, 'node_modules', '.bin');
    return {
        typescript: path.join(bin, `tsc${suffix}`),
        acorn: path.join(bin, `acorn${suffix}`),
    };
}

function appendEnvironment(file, commands) {
    fs.appendFileSync(file,
        `BLUEICE_BLUETSC_ORACLE=${commands.typescript}\n` +
        `BLUEICE_ACORN_ORACLE=${commands.acorn}\n`);
}

function main() {
    if (!process.env.RUNNER_TEMP || !process.env.GITHUB_ENV) {
        throw new Error('RUNNER_TEMP and GITHUB_ENV must be set by GitHub Actions');
    }
    const commands = oracleCommands(path.join(process.env.RUNNER_TEMP, 'bluets-oracles'));
    const ts = loadTypeScript(commands.typescript);
    const acorn = loadAcorn(commands.acorn);
    if (!fs.statSync(loadTypeScript.cliPath(commands.typescript)).isFile()) {
        throw new Error('Pinned TypeScript CLI is missing');
    }
    appendEnvironment(process.env.GITHUB_ENV, commands);
    process.stdout.write(`Pinned TypeScript ${ts.version}, Acorn ${acorn.version}\n`);
}

if (require.main === module) main();
module.exports = { oracleCommands, appendEnvironment };
