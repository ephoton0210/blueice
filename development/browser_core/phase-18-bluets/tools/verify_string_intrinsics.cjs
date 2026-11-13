// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// All frozen verdicts and bytes; actual compiler consumers, syntax and runtime.
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const {spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const {inventory, writeSources, blueDiagnostic} = require('./string_intrinsics/files.cjs');
const makeConsumer = require('./string_intrinsics/consumers.cjs');
const repo = path.resolve(__dirname, '../../../..');
const resolve = require(path.join(repo, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
const implementation = resolve(process.env.BLUEICE_BLUETSC_ORACLE, 'typescript', 'lib/typescript.js');
const ts = require(implementation);
const acorn = require(resolve(process.env.BLUEICE_ACORN_ORACLE, 'acorn', 'dist/acorn.js'));
assert.equal(ts.version, '5.9.3');
assert.equal(acorn.version, '8.15.0');
assert.equal(process.argv.length, 2);
const sha = value => createHash('sha256').update(value).digest('hex');
const directory = path.join(repo, 'backend/bluets/tests/fixtures/string_intrinsics');
const frozen = JSON.parse(fs.readFileSync(path.join(directory, 'FROZEN.json')));
const input = JSON.parse(fs.readFileSync(path.join(directory, 'cases.json')));
const reference = JSON.parse(fs.readFileSync(path.join(directory, 'reference.json')));
const compiler = fs.realpathSync(process.env.BLUEICE_STRING_INTRINSICS_COMPILER);
const compilerSha256 = sha(fs.readFileSync(compiler));
if (process.env.BLUEICE_STRING_INTRINSICS_EXPECTED_COMPILER_SHA256) {
    assert.equal(compilerSha256, process.env.BLUEICE_STRING_INTRINSICS_EXPECTED_COMPILER_SHA256);
}
function verifyAuthorities() {
    for (const [name, digest] of Object.entries(frozen.sha256)) {
        assert.equal(sha(fs.readFileSync(path.join(directory, name))), digest, name + ': frozen bytes changed');
    }
    assert.equal(sha(fs.readFileSync(implementation)), reference.implementationSha256);
    for (const [name, digest] of Object.entries(reference.libraries)) {
        assert.equal(sha(fs.readFileSync(path.join(path.dirname(implementation), name))), digest);
    }
    assert.equal(sha(fs.readFileSync(compiler)), compilerSha256);
}
verifyAuthorities();
assert.equal(input.cases.length, 245);
assert.equal(input.profiles.length, 4);
assert.equal(reference.cases.length, 980);
const expected = new Map(reference.cases.map(row => [row.id, row]));
assert.equal(expected.size, 980);
const capture = process.env.BLUEICE_STRING_INTRINSICS_OBSERVATIONS;
const parent = capture ? path.resolve(capture) : os.tmpdir();
fs.mkdirSync(parent, {recursive: true});
const root = fs.mkdtempSync(path.join(parent, 'bluets-string-intrinsics-blue-'));
const report = {
    schema: 1, sourceRevision: process.env.BLUEICE_STRING_INTRINSICS_SOURCE_REVISION || null,
    compilerSha256, frozenInputs: frozen.sha256,
    implementationSha256: reference.implementationSha256, libraries: reference.libraries,
    cases: [], accepted: 0, rejected: 0, declarationFiles: 0, consumers: 0, syntax: 0,
    runtime: 0, conclusion: 'in_progress',
};
const save = () => fs.writeFileSync(path.join(root, 'report.json'), JSON.stringify(report, null, 2) + '\n');
const consumer = makeConsumer(ts, implementation);
try {
    save();
    for (const profile of input.profiles) for (const entry of input.cases) {
        const id = profile.id + '/' + entry.id, wanted = expected.get(id);
        assert(wanted, id);
        const cwd = path.join(root, profile.id, entry.id);
        fs.mkdirSync(cwd, {recursive: true});
        writeSources(cwd, entry);
        const options = {...input.options, ...profile.compilerOptions, outDir: 'dist'};
        fs.writeFileSync(path.join(cwd, 'tsconfig.json'), JSON.stringify({
            compilerOptions: options, files: ['main.ts', ...Object.keys(entry.files || {})],
        }, null, 2) + '\n');
        const built = spawnSync(compiler, ['build', '--config', 'tsconfig.json', '--diagnostics-json'], {
            cwd, encoding: 'utf8', timeout: 120000,
        });
        fs.writeFileSync(path.join(cwd, 'build.stdout'), built.stdout || '');
        fs.writeFileSync(path.join(cwd, 'build.stderr'), built.stderr || '');
        if (built.error) throw built.error;
        const diagnostics = built.stderr.split(/\r?\n/).filter(Boolean).map(line => blueDiagnostic(JSON.parse(line)));
        const files = inventory(path.join(cwd, 'dist'));
        const declarations = Object.fromEntries(Object.entries(files).filter(([name]) => /\.d\.(ts|mts|cts)$/.test(name)));
        const row = {id, accepts: built.status === 0, diagnostics, declarations};
        fs.writeFileSync(path.join(cwd, 'observation.json'), JSON.stringify(row, null, 2) + '\n');
        assert.equal(row.accepts, wanted.accepts, id + ': verdict differs');
        assert.deepEqual(diagnostics, wanted.diagnostics, id + ': complete diagnostics differ');
        assert.deepEqual(declarations, wanted.declarations, id + ': physical declaration bytes differ');
        if (!wanted.accepts) {
            assert.deepEqual(files, {}, id + ': rejected program published output');
            report.rejected++;
        } else {
            report.accepted++;
            report.declarationFiles += Object.keys(declarations).length;
            const consumerRoot = path.join(cwd, 'consumers');
            fs.mkdirSync(consumerRoot);
            for (const [name, text] of Object.entries(declarations)) {
                const file = path.join(consumerRoot, name);
                fs.mkdirSync(path.dirname(file), {recursive: true});
                fs.writeFileSync(file, text);
            }
            const converted = ts.convertCompilerOptionsFromJson(options, consumerRoot);
            assert.deepEqual(converted.errors, []);
            row.consumers = {};
            for (const polarity of ['positive', 'negative']) {
                const observed = consumer(consumerRoot, converted.options, entry.consumer[polarity], declarations);
                assert.deepEqual(observed, wanted.consumers[polarity], id + ': ' + polarity + ' consumer differs');
                row.consumers[polarity] = observed;
                report.consumers++;
            }
            row.javascriptSha256 = {};
            for (const [name, javascript] of Object.entries(files).filter(([name]) => /\.(js|mjs|cjs)$/.test(name))) {
                acorn.parse(javascript, {
                    ecmaVersion: profile.compilerOptions.target === 'ES5' ? 5 : 2022,
                    sourceType: profile.compilerOptions.module === 'CommonJS' ? 'script' : 'module',
                });
                row.javascriptSha256[name] = sha(javascript);
                report.syntax++;
            }
            if (entry.runtime) {
                const esm = profile.compilerOptions.module === 'ESNext';
                const check = esm ? 'if(runtime!==2)throw Error("runtime result");'
                    : 'if(module.exports.runtime!==2)throw Error("runtime result");';
                const run = spawnSync(process.execPath, ['--input-type=' + (esm ? 'module' : 'commonjs')], {
                    input: files['main.js'] + '\n' + check + '\n', encoding: 'utf8', timeout: 120000,
                });
                if (run.error) throw run.error;
                row.runtime = {exit: run.status, stdout: run.stdout, stderr: run.stderr};
                assert.deepEqual(row.runtime, wanted.runtime, id + ': runtime differs');
                report.runtime++;
            }
        }
        report.cases.push(row);
        save();
    }
    assert.equal(report.cases.length, 980);
    assert.equal(report.accepted, 772);
    assert.equal(report.rejected, 208);
    assert.equal(report.declarationFiles, 804);
    assert.equal(report.consumers, 1544);
    assert.equal(report.syntax, 804);
    assert.equal(report.runtime, 16);
    verifyAuthorities();
    report.conclusion = 'success';
    save();
    console.log(JSON.stringify({root, compilerSha256, comparisons: report.cases.length,
        accepted: report.accepted, declarationFiles: report.declarationFiles,
        consumers: report.consumers, syntax: report.syntax, runtime: report.runtime}));
} catch (error) {
    report.conclusion = 'failure';
    report.error = String(error.stack || error);
    save();
    throw error;
}
