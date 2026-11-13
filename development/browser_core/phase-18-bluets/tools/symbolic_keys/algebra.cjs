// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independent supplemental key-algebra controls, with zero inventory credit.
const {spawnSync} = require('node:child_process');
const {isDeepStrictEqual} = require('node:util');
const c = require('./common.cjs');
const {assert, fs, path, fixture, sha, read} = c;

module.exports = function verify(mode, ctx, compiler) {
    assert(['native', 'blue'].includes(mode));
    const inputPath = path.join(fixture, 'algebra-controls.json');
    const inputBytes = fs.readFileSync(inputPath), input = JSON.parse(inputBytes);
    const frozen = read(path.join(fixture, 'FROZEN.json'));
    const compilerSha256 = compiler ? sha(fs.readFileSync(compiler)) : null;
    const authority = () => {
        assert.equal(sha(fs.readFileSync(inputPath)), frozen.supplementarySha256['algebra-controls.json']);
        assert.equal(sha(fs.readFileSync(ctx.implementation)), input.implementationSha256);
        for (const [name, digest] of Object.entries(input.libraries))
            assert.equal(sha(fs.readFileSync(path.join(path.dirname(ctx.implementation), name))), digest, name);
        if (compiler) assert.equal(sha(fs.readFileSync(compiler)), compilerSha256);
    };
    authority();
    assert.equal(input.cases.length, 16);
    const root = c.capture('algebra-' + mode), loadedLibraries = {}, cache = new Map();
    const report = {schema: 1, scope: input.scope, inventoryCredit: 0, mode,
        sourceRevision: process.env.BLUEICE_SYMBOLIC_KEYS_SOURCE_REVISION || null,
        inputSha256: sha(inputBytes), implementationSha256: input.implementationSha256,
        libraries: input.libraries, compilerSha256, cases: [], conclusion: 'in_progress'};
    const save = () => fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n');
    try {
        save();
        for (const entry of input.cases) {
            const cwd = path.join(root, entry.id);
            fs.mkdirSync(cwd);
            fs.writeFileSync(path.join(cwd, 'main.ts'), entry.source);
            fs.writeFileSync(path.join(cwd, 'tsconfig.json'), JSON.stringify({compilerOptions: input.options,
                files: ['main.ts']}, null, 2) + '\n');
            let accepts, diagnostics;
            if (mode === 'native') {
                const converted = ctx.ts.convertCompilerOptionsFromJson(input.options, cwd);
                assert.deepEqual(converted.errors, []);
                const host = ctx.ts.createCompilerHost(converted.options), get = host.getSourceFile;
                host.getSourceFile = (file, version, ...rest) => {
                    const relative = path.relative(path.dirname(ctx.implementation), file);
                    if (relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) return get(file, version, ...rest);
                    const key = file + ':' + version;
                    if (!cache.has(key)) cache.set(key, get(file, version, ...rest));
                    loadedLibraries[path.basename(file)] = sha(fs.readFileSync(file));
                    return cache.get(key);
                };
                const program = ctx.ts.createProgram([path.join(cwd, 'main.ts')], converted.options, host);
                diagnostics = ctx.ts.sortAndDeduplicateDiagnostics(ctx.ts.getPreEmitDiagnostics(program))
                    .map(row => c.diagnostic(ctx.ts, row, cwd));
                accepts = diagnostics.length === 0;
                if (accepts) {
                    const emitted = program.emit();
                    assert(!emitted.emitSkipped && emitted.diagnostics.length === 0);
                }
            } else {
                const built = spawnSync(compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json'], {
                    cwd, encoding: 'utf8', timeout: 120000,
                });
                if (built.error) throw built.error;
                assert.equal(built.signal, null, entry.id + ': compiler terminated by signal');
                fs.writeFileSync(path.join(cwd, 'build.stdout'), built.stdout || '');
                fs.writeFileSync(path.join(cwd, 'build.stderr'), built.stderr || '');
                accepts = built.status === 0;
                diagnostics = built.stderr.split(/\r?\n/).filter(Boolean).map(line => c.blueDiagnostic(JSON.parse(line)));
            }
            const outputs = c.inventory(path.join(cwd, 'dist'));
            const declarations = Object.fromEntries(Object.entries(outputs).filter(([name]) => name.endsWith('.d.ts')));
            const observed = {accepts, diagnostics, declarations}, javascriptSha256 = {};
            if (accepts) {
                for (const [name, source] of Object.entries(outputs).filter(([name]) => name.endsWith('.js'))) {
                    ctx.acorn.parse(source, {ecmaVersion: 2022, sourceType: 'script'});
                    javascriptSha256[name] = sha(source);
                }
                assert.equal(Object.keys(javascriptSha256).length, 1, entry.id + ': executable output required');
            }
            const publishedOnRejectedNativeInput = !entry.expected.accepts && Object.keys(outputs).length > 0;
            const row = {id: entry.id, observed, javascriptSha256, publishedOnRejectedNativeInput,
                agrees: isDeepStrictEqual(observed, entry.expected) && !publishedOnRejectedNativeInput};
            fs.writeFileSync(path.join(cwd, 'observation.json'), JSON.stringify(row, null, 2) + '\n');
            report.cases.push(row); save();
        }
        if (mode === 'native') assert.deepEqual(loadedLibraries, input.libraries);
        authority();
        const differences = report.cases.filter(row => !row.agrees).map(row => row.id);
        assert.deepEqual(differences, [], 'complete supplementary Native observations differ');
        report.accepted = report.cases.filter(row => row.observed.accepts).length;
        report.rejected = report.cases.length - report.accepted;
        report.syntax = report.cases.reduce((n, row) => n + Object.keys(row.javascriptSha256).length, 0);
        assert.equal(report.accepted, 15); assert.equal(report.rejected, 1); assert.equal(report.syntax, 15);
        report.conclusion = 'success'; save();
        console.log(JSON.stringify({root, mode, comparisons: report.cases.length, syntax: report.syntax,
            inventoryCredit: report.inventoryCredit, compilerSha256}));
        return report;
    } catch (error) {
        report.conclusion = 'failure'; report.error = String(error.stack || error); save(); throw error;
    }
};
