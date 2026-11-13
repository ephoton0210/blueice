// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), os = require('node:os');
const {spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const {inventory, writeSources, blueDiagnostic} = require('../string_intrinsics/files.cjs');
const makeConsumer = require('../string_intrinsics/consumers.cjs');
const repo = path.resolve(__dirname, '../../../../..');
const fixture = path.join(repo, 'backend/bluets/tests/fixtures/symbolic_keys');
const sha = value => createHash('sha256').update(value).digest('hex');
const read = file => JSON.parse(fs.readFileSync(file));

function context() {
    const resolve = require(path.join(repo, 'backend/bluets/tests/fixtures/oracle_support/resolve_package.cjs'));
    const implementation = resolve(process.env.BLUEICE_BLUETSC_ORACLE, 'typescript', 'lib/typescript.js');
    const ts = require(implementation);
    const acorn = require(resolve(process.env.BLUEICE_ACORN_ORACLE, 'acorn', 'dist/acorn.js'));
    assert.equal(ts.version, '5.9.3');
    assert.equal(acorn.version, '8.15.0');
    return {ts, acorn, implementation, consumer: makeConsumer(ts, implementation)};
}

function capture(label) {
    const parent = process.env.BLUEICE_SYMBOLIC_KEYS_OBSERVATIONS || os.tmpdir();
    fs.mkdirSync(parent, {recursive: true});
    return fs.mkdtempSync(path.join(path.resolve(parent), 'bluets-symbolic-keys-' + label + '-'));
}

function diagnostic(ts, value, root) {
    let file = null, position = null;
    if (value.file && value.start !== undefined) {
        const at = value.file.getLineAndCharacterOfPosition(value.start);
        file = path.relative(root, value.file.fileName).replaceAll('\\', '/');
        position = {line: at.line + 1, column: at.character + 1, length: value.length};
    }
    return {code: value.code, message: ts.flattenDiagnosticMessageText(value.messageText, '\n'),
        file, position, relatedInformation: (value.relatedInformation || []).map(row => diagnostic(ts, row, root))};
}

function prepare(root, input, profile, entry) {
    const cwd = path.join(root, profile.id, entry.id);
    fs.mkdirSync(cwd, {recursive: true});
    writeSources(cwd, entry);
    const options = {...input.options, ...profile.compilerOptions, ...entry.compilerOptions, outDir: 'dist'};
    const config = {compilerOptions: options, files: ['main.ts', ...Object.keys(entry.files || {})]};
    fs.writeFileSync(path.join(cwd, 'tsconfig.json'), JSON.stringify(config, null, 2) + '\n');
    return {cwd, options, config};
}

function downstream(ctx, cwd, profile, entry, options, outputs) {
    const declarations = Object.fromEntries(Object.entries(outputs).filter(([name]) => /\.d\.(ts|mts|cts)$/.test(name)));
    assert.deepEqual(Object.keys(entry.consumerSources || {}).sort(), ['negative', 'positive'], entry.id + ': authored consumers required');
    const converted = ctx.ts.convertCompilerOptionsFromJson(options, cwd);
    assert.deepEqual(converted.errors, []);
    const consumers = {};
    for (const polarity of ['positive', 'negative']) {
        const root = path.join(cwd, 'consumers', polarity);
        fs.mkdirSync(root, {recursive: true});
        for (const [name, text] of Object.entries(declarations)) {
            const file = path.join(root, name);
            fs.mkdirSync(path.dirname(file), {recursive: true});
            fs.writeFileSync(file, text);
        }
        consumers[polarity] = ctx.consumer(root, converted.options, entry.consumerSources[polarity], declarations);
        assert.equal(consumers[polarity].accepts, polarity === 'positive', entry.id + ': ' + polarity + ' consumer polarity');
    }
    const javascriptSha256 = {};
    for (const [name, source] of Object.entries(outputs).filter(([name]) => /\.(js|mjs|cjs)$/.test(name))) {
        ctx.acorn.parse(source, {ecmaVersion: profile.compilerOptions.target === 'ES5' ? 5 : 2022,
            sourceType: profile.compilerOptions.module === 'CommonJS' ? 'script' : 'module'});
        javascriptSha256[name] = sha(source);
    }
    const row = {consumers, javascriptSha256};
    if (entry.runtime) {
        const esm = profile.compilerOptions.module === 'ESNext';
        const code = outputs['main.js'] + '\nvar check = ' + (esm ? 'result' : 'module.exports.result') + ';\n' + entry.runtime + '\n';
        const result = spawnSync(process.execPath, ['--input-type=' + (esm ? 'module' : 'commonjs')], {
            input: code, encoding: 'utf8', timeout: 120000,
        });
        if (result.error) throw result.error;
        row.runtime = {exit: result.status, stdout: result.stdout, stderr: result.stderr};
        fs.writeFileSync(path.join(cwd, 'runtime-check.js'), code);
        assert.equal(result.status, 0, entry.id + ': runtime: ' + result.stderr);
    }
    return row;
}

function counts(rows) {
    return {comparisons: rows.length, accepted: rows.filter(r => r.accepts).length,
        rejected: rows.filter(r => !r.accepts).length,
        declarationFiles: rows.reduce((n, r) => n + Object.keys(r.declarations).length, 0),
        consumers: rows.reduce((n, r) => n + Object.keys(r.consumers || {}).length, 0),
        syntax: rows.reduce((n, r) => n + Object.keys(r.javascriptSha256 || {}).length, 0),
        runtime: rows.filter(r => r.runtime).length};
}

module.exports = {assert, fs, path, repo, fixture, sha, read, inventory, blueDiagnostic,
    context, capture, diagnostic, prepare, downstream, counts};
