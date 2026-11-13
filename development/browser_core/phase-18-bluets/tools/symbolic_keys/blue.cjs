// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const {spawnSync} = require('node:child_process');
const c = require('./common.cjs');
const {assert, fs, path, fixture, sha, read} = c;

module.exports = function verify() {
    assert.equal(process.argv.length, 2);
    const input = read(path.join(fixture, 'cases.json')), reference = read(path.join(fixture, 'reference.json'));
    const frozen = read(path.join(fixture, 'FROZEN.json'));
    const compiler = fs.realpathSync(process.env.BLUEICE_SYMBOLIC_KEYS_COMPILER);
    const compilerSha256 = sha(fs.readFileSync(compiler));
    if (process.env.BLUEICE_SYMBOLIC_KEYS_EXPECTED_COMPILER_SHA256)
        assert.equal(compilerSha256, process.env.BLUEICE_SYMBOLIC_KEYS_EXPECTED_COMPILER_SHA256);
    const ctx = c.context(), root = c.capture('blue');
    const expected = new Map(reference.cases.map(row => [row.id, row]));
    assert.equal(expected.size, input.cases.length * input.profiles.length);
    function authorities() {
        for (const [name, digest] of Object.entries({...frozen.sha256, ...frozen.supplementarySha256}))
            assert.equal(sha(fs.readFileSync(path.join(fixture, name))), digest, name);
        assert.equal(sha(fs.readFileSync(ctx.implementation)), reference.implementationSha256);
        for (const [name, digest] of Object.entries(reference.libraries))
            assert.equal(sha(fs.readFileSync(path.join(path.dirname(ctx.implementation), name))), digest, name);
        assert.equal(sha(fs.readFileSync(compiler)), compilerSha256);
    }
    authorities();
    const report = {schema: 1, sourceRevision: process.env.BLUEICE_SYMBOLIC_KEYS_SOURCE_REVISION || null,
        compilerSha256, frozenInputs: frozen.sha256, implementationSha256: reference.implementationSha256,
        libraries: reference.libraries, cases: [], conclusion: 'in_progress'};
    const save = () => fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n');
    try {
        save();
        for (const profile of input.profiles) for (const entry of input.cases) {
            const id = profile.id + '/' + entry.id, wanted = expected.get(id);
            const {cwd, options} = c.prepare(root, input, profile, entry);
            const result = spawnSync(compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json'], {
                cwd, encoding: 'utf8', timeout: 120000,
            });
            if (result.error) throw result.error;
            fs.writeFileSync(path.join(cwd, 'build.stdout'), result.stdout || '');
            fs.writeFileSync(path.join(cwd, 'build.stderr'), result.stderr || '');
            const diagnostics = result.stderr.split(/\r?\n/).filter(Boolean).map(line => c.blueDiagnostic(JSON.parse(line)));
            const outputs = c.inventory(path.join(cwd, 'dist'));
            const declarations = Object.fromEntries(Object.entries(outputs).filter(([name]) => /\.d\.(ts|mts|cts)$/.test(name)));
            const row = {id, accepts: result.status === 0, diagnostics, declarations};
            fs.writeFileSync(path.join(cwd, 'observation.json'), JSON.stringify(row, null, 2) + '\n');
            assert.equal(row.accepts, wanted.accepts, id + ': verdict differs');
            assert.deepEqual(row.diagnostics, wanted.diagnostics, id + ': full diagnostics differ');
            assert.deepEqual(row.declarations, wanted.declarations, id + ': exact declaration bytes differ');
            if (row.accepts) {
                const checked = c.downstream(ctx, cwd, profile, entry, options, outputs);
                assert.deepEqual(checked.consumers, wanted.consumers, id + ': declaration consumers differ');
                if (wanted.runtime) assert.deepEqual(checked.runtime, wanted.runtime, id + ': runtime differs');
                Object.assign(row, checked);
            } else assert.deepEqual(outputs, {}, id + ': rejected program emitted output');
            report.cases.push(row); save();
        }
        const actual = c.counts(report.cases), expectedCounts = c.counts(reference.cases);
        assert.deepEqual(actual, expectedCounts);
        const algebra = require('./algebra.cjs')('blue', ctx, compiler);
        report.supplementary = {comparisons: algebra.cases.length, syntax: algebra.syntax, inventoryCredit: 0};
        authorities(); Object.assign(report, actual, {conclusion: 'success'}); save();
        console.log(JSON.stringify({root, compilerSha256, ...actual}));
    } catch (error) {
        report.conclusion = 'failure'; report.error = String(error.stack || error); save(); throw error;
    }
};
