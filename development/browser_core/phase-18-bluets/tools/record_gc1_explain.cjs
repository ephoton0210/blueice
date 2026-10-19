// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Freeze complete explainFiles output; no filtering of files or reason lines.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '../../../..');
const load = require(path.join(root, 'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'));
const command = process.env.BLUEICE_BLUETSC_ORACLE || 'tsc';
const ts = load(command);
const directory = path.join(root, 'backend/bluets/tests/fixtures/gc1_explain');
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json'), 'utf8'));
if (input.schema !== 1 || input.typescript !== ts.version) throw new Error('Foreign explainFiles matrix');
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-gc1-explain-'));
try {
    const cases = input.cases.map(row => {
        const project = path.join(temporary, row.id);
        fs.mkdirSync(project);
        for (const [name, source] of Object.entries(row.files)) {
            const target = path.resolve(project, name);
            if (!target.startsWith(project + path.sep)) throw new Error('Escaping fixture source');
            fs.mkdirSync(path.dirname(target), {recursive: true});
            fs.writeFileSync(target, source.replace('@globals', input.globals));
        }
        const config = {...row.config, compilerOptions: {...input.options, ...row.config.compilerOptions}};
        fs.writeFileSync(path.join(project, 'tsconfig.json'), JSON.stringify(config, null, 2) + '\n');
        const args = [load.cliPath(command), '--project', 'tsconfig.json', '--pretty', 'false', '--explainFiles'];
        const result = spawnSync(process.execPath, args, {cwd: project, encoding: 'utf8', env: {...process.env, FORCE_COLOR: '0'}});
        if (result.error || result.status !== 0 || result.stderr) throw new Error(`${row.id}: ${result.error || result.stdout + result.stderr}`);
        const normalize = text => text.replaceAll('\\', '/').replaceAll(project.replaceAll('\\', '/'), '<project>').replaceAll('\r\n', '\n');
        return {id: row.id, exit: result.status, stdout: normalize(result.stdout), stderr: normalize(result.stderr)};
    });
    const text = JSON.stringify({schema: 1, typescript: ts.version, cases}, null, 2) + '\n';
    const output = path.join(directory, 'reference.json');
    if (process.env.BLUEICE_WRITE_GC1_EXPLAIN_MATRIX === '1') fs.writeFileSync(output, text);
    else if (fs.readFileSync(output, 'utf8').replaceAll('\r\n', '\n') !== text) throw new Error('explainFiles oracle changed');
    process.stdout.write(`Verified ${cases.length} complete TypeScript ${ts.version} explainFiles observations\n`);
} finally {
    fs.rmSync(temporary, {recursive: true, force: true});
}
