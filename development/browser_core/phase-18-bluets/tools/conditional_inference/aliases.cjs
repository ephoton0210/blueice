// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const {spawnSync} = require('node:child_process');
const c = require('./common.cjs');
const {assert, fs, path, fixture, sha, read} = c;

module.exports = function aliases(mode, ctx, compiler = null) {
    assert(['native', 'blue'].includes(mode));
    const inputPath = path.join(fixture, 'alias-controls.json'), input = read(inputPath);
    c.authorities(ctx);
    const root = c.capture('alias-' + mode), compilerSha256 = compiler ? sha(fs.readFileSync(compiler)) : null;
    const rows = [];
    for (const entry of input.cases) {
        const cwd = path.join(root, entry.id);
        fs.mkdirSync(cwd, {recursive: true});
        fs.writeFileSync(path.join(cwd, 'main.ts'), entry.source);
        const config = {compilerOptions: entry.options, files: ['main.ts']};
        fs.writeFileSync(path.join(cwd, 'tsconfig.json'), JSON.stringify(config, null, 2) + '\n');
        let accepts, diagnostics, outputs;
        if (mode === 'native') {
            const observed = c.nativeProject(ctx, cwd, config);
            diagnostics = observed.diagnostics; accepts = diagnostics.length === 0; outputs = observed.outputs;
        } else {
            const observed = spawnSync(compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json'],
                {cwd, encoding: 'utf8', timeout: 120000});
            if (observed.error) throw observed.error;
            fs.writeFileSync(path.join(cwd, 'build.stdout'), observed.stdout || '');
            fs.writeFileSync(path.join(cwd, 'build.stderr'), observed.stderr || '');
            accepts = observed.status === 0;
            diagnostics = observed.stderr.split(/\r?\n/).filter(Boolean).map(line => c.blueDiagnostic(JSON.parse(line)));
            outputs = c.inventory(path.join(cwd, 'dist'));
        }
        const actual = {accepts, diagnostics,
            declarations: Object.fromEntries(Object.entries(outputs).filter(([name]) => name.endsWith('.d.ts')))};
        assert.deepEqual(actual, entry.expected, entry.id + ': complete alias observation differs');
        const javascriptSha256 = {};
        for (const [name, text] of Object.entries(outputs).filter(([name]) => name.endsWith('.js'))) {
            ctx.acorn.parse(text, {ecmaVersion: entry.options.target === 'ES5' ? 5 : 2022,
                sourceType: entry.options.module === 'CommonJS' ? 'script' : 'module'});
            javascriptSha256[name] = sha(text);
        }
        if (mode === 'native') assert.deepEqual(javascriptSha256, entry.javascriptSha256);
        rows.push({id: entry.id, observed: actual, agrees: true, javascriptSha256});
    }
    const report = {mode, sourceRevision: process.env.BLUEICE_CONDITIONAL_INFERENCE_SOURCE_REVISION || null,
        compilerSha256, inputSha256: sha(fs.readFileSync(inputPath)),
        implementationSha256: sha(fs.readFileSync(ctx.implementation)), cases: rows,
        comparisons: rows.length, syntax: rows.reduce((n, row) => n + Object.keys(row.javascriptSha256).length, 0),
        inventoryCredit: 0, conclusion: 'success'};
    assert.equal(report.comparisons, 24); assert.equal(report.syntax, 24);
    fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n');
    c.authorities(ctx);
    if (compiler) assert.equal(sha(fs.readFileSync(compiler)), compilerSha256);
    return report;
};
