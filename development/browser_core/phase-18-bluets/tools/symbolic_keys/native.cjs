// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const c = require('./common.cjs');
const {assert, fs, path, fixture, sha, read} = c;

module.exports = function record() {
    const verify = process.argv[2] === '--verify';
    assert.deepEqual(process.argv.slice(2), verify ? ['--verify'] : []);
    const referencePath = path.join(fixture, 'reference.json');
    const original = verify ? fs.readFileSync(referencePath) : null;
    if (!verify) assert(!fs.existsSync(referencePath), 'never overwrite frozen Native expectations');
    const inputBytes = fs.readFileSync(path.join(fixture, 'cases.json')), input = JSON.parse(inputBytes);
    const ctx = c.context(), root = c.capture('native');
    const libraries = {}, cache = new Map(), rows = [], authoringErrors = [];
    for (const profile of input.profiles) for (const entry of input.cases) {
        const {cwd, options, config} = c.prepare(root, input, profile, entry);
        const converted = ctx.ts.convertCompilerOptionsFromJson(options, cwd);
        assert.deepEqual(converted.errors, []);
        const host = ctx.ts.createCompilerHost(converted.options), get = host.getSourceFile;
        host.getSourceFile = (file, version, ...rest) => {
            const relative = path.relative(path.dirname(ctx.implementation), file);
            if (relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) return get(file, version, ...rest);
            const key = file + ':' + version;
            if (!cache.has(key)) cache.set(key, get(file, version, ...rest));
            libraries[path.basename(file)] = sha(fs.readFileSync(file));
            return cache.get(key);
        };
        host.writeFile = (file, text) => {
            const relative = path.relative(path.join(cwd, 'dist'), file);
            assert(!path.isAbsolute(relative) && !relative.split(path.sep).includes('..'));
            fs.mkdirSync(path.dirname(file), {recursive: true});
            fs.writeFileSync(file, text);
        };
        const program = ctx.ts.createProgram(config.files.map(file => path.join(cwd, file)), converted.options, host);
        const diagnostics = ctx.ts.sortAndDeduplicateDiagnostics(ctx.ts.getPreEmitDiagnostics(program))
            .map(row => c.diagnostic(ctx.ts, row, cwd));
        if (!diagnostics.length) {
            const result = program.emit();
            assert(!result.emitSkipped && result.diagnostics.length === 0);
        }
        const outputs = c.inventory(path.join(cwd, 'dist'));
        const declarations = Object.fromEntries(Object.entries(outputs).filter(([name]) => /\.d\.(ts|mts|cts)$/.test(name)));
        const row = {id: profile.id + '/' + entry.id, form: entry.id, profile: profile.id,
            accepts: !diagnostics.length, diagnostics, declarations};
        if (row.accepts) {
            try { Object.assign(row, c.downstream(ctx, cwd, profile, entry, options, outputs)); }
            catch (error) { authoringErrors.push({id: row.id, error: String(error.stack || error)}); }
        } else assert.deepEqual(outputs, {}, row.id + ': rejected Native input emitted output');
        fs.writeFileSync(path.join(cwd, 'observation.json'), JSON.stringify(row, null, 2) + '\n');
        rows.push(row);
    }
    const report = {schema: 1, typescript: ctx.ts.version, node: process.version,
        implementationSha256: sha(fs.readFileSync(ctx.implementation)), libraries,
        inputSha256: sha(inputBytes), forms: input.cases.length, profiles: input.profiles.length,
        ...c.counts(rows), cases: rows, authoringErrors,
        conclusion: authoringErrors.length ? 'authoring-errors' : 'independently-calibrated'};
    fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n');
    if (authoringErrors.length) {
        console.error(JSON.stringify(authoringErrors, null, 2));
        process.exitCode = 1;
    } else if (verify) {
        // Node-version provenance may differ; every measured semantic/output
        // field, including JavaScript bytes, remains part of the comparison.
        const comparable = value => {
            const {node, ...result} = value;
            return result;
        };
        assert.deepEqual(comparable(report), comparable(JSON.parse(original)));
        assert.deepEqual(fs.readFileSync(referencePath), original);
    } else fs.writeFileSync(referencePath, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
    console.log(JSON.stringify({root, conclusion: report.conclusion, ...c.counts(rows),
        inputSha256: report.inputSha256, authoringErrors: authoringErrors.length}));
    if (verify && !authoringErrors.length) require('./algebra.cjs')('native', ctx);
};
