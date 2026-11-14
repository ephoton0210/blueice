// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const base = require('../conditional_inference/common.cjs');
const {spawnSync} = require('node:child_process');
const os = require('node:os');
const {assert, fs, path, repo, sha, read} = base;
const fixture = path.join(repo, 'backend/bluets/tests/fixtures/higher_order_inference');

function capture(label) {
    const parent = process.env.BLUEICE_HIGHER_ORDER_INFERENCE_OBSERVATIONS || os.tmpdir();
    fs.mkdirSync(parent, {recursive:true});
    return fs.mkdtempSync(path.join(path.resolve(parent), 'bluets-higher-order-inference-' + label + '-'));
}

function authorities(ctx) {
    const frozen = read(path.join(fixture, 'FROZEN.json'));
    for (const [name, digest] of Object.entries(frozen.sha256))
        assert.equal(sha(fs.readFileSync(path.join(fixture, name))), digest, name);
    assert.equal(sha(fs.readFileSync(ctx.implementation)), frozen.implementationSha256);
    for (const [name, digest] of Object.entries(frozen.libraries))
        assert.equal(sha(fs.readFileSync(path.join(path.dirname(ctx.implementation), name))), digest, name);
    return frozen;
}

function downstream(ctx, cwd, profile, entry, options, outputs) {
    const checked = base.downstream(ctx, cwd, profile, {...entry, runtime:undefined}, options, outputs);
    if (!entry.runtime) return checked;
    const directory = path.join(cwd, 'runtime');
    fs.mkdirSync(directory, {recursive:true});
    for (const [name, text] of Object.entries(outputs).filter(([name]) => /\.(js|mjs|cjs)$/.test(name))) {
        const destination = path.join(directory, name);
        fs.mkdirSync(path.dirname(destination), {recursive:true});
        fs.writeFileSync(destination, text);
    }
    const esm = profile.compilerOptions.module === 'ESNext';
    fs.writeFileSync(path.join(directory, 'package.json'), JSON.stringify({type:esm?'module':'commonjs'}) + '\n');
    const runner = path.join(directory, esm?'check.mjs':'check.cjs');
    const source = (esm?'import { result as check } from "./main.js";\n':'var check = require("./main.js").result;\n') + entry.runtime;
    fs.writeFileSync(runner, source);
    const result = spawnSync(process.execPath, [runner], {cwd:directory,encoding:'utf8',timeout:120000});
    if (result.error) throw result.error;
    checked.runtime = {exit:result.status, stdout:result.stdout, stderr:result.stderr};
    assert.equal(result.status, 0, entry.id + ': execution: ' + result.stderr);
    return checked;
}

module.exports = {...base, fixture, capture, authorities, downstream};
