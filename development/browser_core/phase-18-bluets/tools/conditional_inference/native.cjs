// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const c = require('./common.cjs');
const {assert, fs, path, fixture, sha, read} = c;

module.exports = function record() {
    assert.deepEqual(process.argv.slice(2), ['--verify'], 'frozen Native expectations are verification-only');
    const original = fs.readFileSync(path.join(fixture, 'reference.json'));
    const inputBytes = fs.readFileSync(path.join(fixture, 'cases.json')), input = JSON.parse(inputBytes);
    const ctx = c.context(), root = c.capture('native'), libraries = {}, rows = [];
    c.authorities(ctx);
    for (const profile of input.profiles) for (const entry of input.cases) {
        const {cwd, options, config} = c.prepare(root, input, profile, entry);
        const native = c.nativeProject(ctx, cwd, config);
        Object.assign(libraries, native.libraries);
        const accepts = native.diagnostics.length === 0;
        const row = {id: profile.id + '/' + entry.id, accepts, diagnostics: native.diagnostics,
            declarations: Object.fromEntries(Object.entries(native.outputs).filter(([name]) => name.endsWith('.d.ts'))),
            authoredVerdictAgrees: accepts === (entry.nativeExpectedAcceptsByProfile?.[profile.id] ?? entry.nativeExpectedAccepts)};
        if (accepts) Object.assign(row, c.downstream(ctx, cwd, profile, entry, options, native.outputs));
        else assert.deepEqual(native.outputs, {}, row.id + ': rejected input published artifacts');
        fs.writeFileSync(path.join(cwd, 'observation.json'), JSON.stringify(row, null, 2) + '\n');
        rows.push(row);
    }
    const report = {schema: 1, typescript: ctx.ts.version,
        implementationSha256: sha(fs.readFileSync(ctx.implementation)), libraries,
        inputSha256: sha(inputBytes), ...c.counts(rows),
        verdictCalibrationQuestions: rows.filter(row => !row.authoredVerdictAgrees).map(row => row.id), cases: rows};
    fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n');
    assert.deepEqual(report.verdictCalibrationQuestions, []);
    assert.deepEqual(report, JSON.parse(original));
    assert.deepEqual(fs.readFileSync(path.join(fixture, 'reference.json')), original);
    require('./aliases.cjs')('native', ctx);
    c.authorities(ctx);
    console.log(JSON.stringify({root, ...c.counts(rows), conclusion: 'success'}));
};
