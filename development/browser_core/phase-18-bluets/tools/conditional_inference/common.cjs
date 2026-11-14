// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const base = require('../symbolic_keys/common.cjs');
const os = require('node:os');
const {assert, fs, path, repo, sha, read} = base;
const fixture = path.join(repo, 'backend/bluets/tests/fixtures/conditional_inference');

function capture(label) {
    const parent = process.env.BLUEICE_CONDITIONAL_INFERENCE_OBSERVATIONS || os.tmpdir();
    fs.mkdirSync(parent, {recursive: true});
    return fs.mkdtempSync(path.join(path.resolve(parent), 'bluets-conditional-inference-' + label + '-'));
}

function authorities(ctx) {
    const frozen = read(path.join(fixture, 'FROZEN.json'));
    const extra = read(path.join(fixture, 'ALIAS_CONTROLS_FROZEN.json'));
    for (const [name, digest] of Object.entries({...frozen.sha256, ...extra.sha256}))
        assert.equal(sha(fs.readFileSync(path.join(fixture, name))), digest, name);
    assert.equal(sha(fs.readFileSync(ctx.implementation)), frozen.implementationSha256);
    assert.equal(frozen.implementationSha256, extra.implementationSha256);
    for (const [name, digest] of Object.entries(frozen.libraries))
        assert.equal(sha(fs.readFileSync(path.join(path.dirname(ctx.implementation), name))), digest, name);
    return frozen;
}

function nativeProject(ctx, cwd, config) {
    const parsed = ctx.ts.parseJsonConfigFileContent(config, ctx.ts.sys, cwd);
    assert.deepEqual(parsed.errors, []);
    const program = ctx.ts.createProgram(parsed.fileNames, parsed.options);
    const before = ctx.ts.getPreEmitDiagnostics(program), emitted = program.emit();
    const diagnostics = ctx.ts.sortAndDeduplicateDiagnostics([...before, ...emitted.diagnostics])
        .map(row => base.diagnostic(ctx.ts, row, cwd));
    const libraries = {};
    for (const source of program.getSourceFiles()) {
        const relative = path.relative(path.dirname(ctx.implementation), source.fileName);
        if (source.isDeclarationFile && !path.isAbsolute(relative)
            && relative !== '..' && !relative.startsWith('..' + path.sep))
            libraries[path.basename(source.fileName)] = sha(fs.readFileSync(source.fileName));
    }
    return {diagnostics, libraries, outputs: base.inventory(path.join(cwd, 'dist'))};
}

module.exports = {...base, fixture, capture, authorities, nativeProject};
