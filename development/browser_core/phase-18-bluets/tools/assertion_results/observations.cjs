// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const c = require('./common.cjs');
const {spawnSync} = require('node:child_process');
const {fs, path, assert, sha} = c;

function build(compiler, directory, surface, root, key) {
    const owner = path.join(root, 'owner-state', key);
    fs.mkdirSync(owner, {recursive:true, mode:0o700});
    const args = surface === 'native' ? ['--project','tsconfig.json','--diagnostics-json'] :
        ['build','--config','tsconfig.json','--diagnostics-json'];
    const result = spawnSync(compiler, args, {cwd:directory, encoding:'utf8', timeout:30000,
        env:{...process.env, BLUETSC_OWNER_STATE_DIR:owner}});
    fs.writeFileSync(path.join(directory, 'build.stdout'), result.stdout || '');
    fs.writeFileSync(path.join(directory, 'build.stderr'), result.stderr || '');
    if (result.error) throw result.error;
    assert.equal(result.signal, null);
    assert([0,1,2].includes(result.status), 'compiler terminated unexpectedly');
    const raw = (result.stderr || '').split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
    const errors = raw.filter(row => row.severity === 'error');
    return {accepts:result.status === 0, exitCode:result.status, rawDiagnostics:raw,
        diagnostics:errors.filter(row => row.typescript).map(c.blueDiagnostic),
        blueOnlyDiagnostics:errors.filter(row => !row.typescript)};
}

function policyRefusal(raw, site, prefix) {
    const offset = Buffer.byteLength(prefix);
    const errors = raw.filter(row => row.severity === 'error');
    return errors.length > 0 && errors.every(row => {
        const span = row.span || {};
        const policy = row.typescript ? row.typescript.code === 2540 :
            row.btsCode === 'BTS3003' && /readonly|opaque/.test(row.rawMessage || '');
        return policy && path.basename(String(span.module || '').replaceAll('\\','/')) === site.file
            && span.start < offset + site.endByte && span.end > offset + site.startByte;
    });
}

function consumer(ctx, compiler, root, input, wanted, surface, profile, producer, engine,
    expected, graph) {
    const directory = path.join(root, 'consumers', surface, producer, engine, wanted.id, expected.polarity);
    const options = {...input.options, ...profile.compilerOptions};
    const source = expected.source;
    assert.equal(sha(source), expected.sourceSha256, 'frozen consumer source');
    c.consumerDirectory(directory, options, source, graph);
    let actual;
    if (engine === 'native') {
        const converted = ctx.ts.convertCompilerOptionsFromJson(options, directory);
        assert.deepEqual(converted.errors, []);
        actual = ctx.consumer(directory, converted.options, source, graph);
    } else {
        const result = build(compiler, directory, surface, root,
            ['consumer',surface,producer,engine,wanted.id,expected.polarity].join('/'));
        assert.deepEqual(result.blueOnlyDiagnostics, [], 'unmatched consumer diagnostics');
        assert.deepEqual(c.inventory(path.join(directory,'dist')), {}, 'noEmit consumer published output');
        actual = {accepts:result.accepts, diagnostics:result.diagnostics};
    }
    fs.writeFileSync(path.join(directory, 'observation.json'), JSON.stringify(actual,null,2)+'\n');
    assert.deepEqual(actual, {accepts:expected.accepts, diagnostics:expected.diagnostics},
        wanted.id+': '+surface+' '+producer+'/'+engine+' '+expected.polarity);
    return {id:wanted.id, surface, producer, engine, polarity:expected.polarity,
        sourceSha256:expected.sourceSha256, observation:path.relative(root,path.join(directory,'observation.json')), ...actual};
}

function runtime(root, wanted, surface, profile, outputs, expected) {
    const directory = path.join(root, 'runtime', surface, wanted.id);
    fs.mkdirSync(directory, {recursive:true});
    const products = {};
    for (const [name,text] of Object.entries(outputs).filter(([name]) => name.endsWith('.js'))) {
        const file = path.join(directory,name);
        fs.mkdirSync(path.dirname(file), {recursive:true});
        fs.writeFileSync(file,text);
        assert.equal(sha(fs.readFileSync(file)), sha(text), 'unmodified product');
        products[name] = sha(text);
    }
    const esm = profile.compilerOptions.module === 'ESNext';
    fs.writeFileSync(path.join(directory, 'package.json'), JSON.stringify({type:esm?'module':'commonjs'})+'\n');
    const source = 'globalThis.value = 41;\n'+
        (esm ? 'const namespace = await import("./main.js");\n' : 'const namespace = require("./main.js");\n')+
        'process.stdout.write(JSON.stringify(namespace.result)+"\\n");\n';
    assert.equal(sha(source), expected.runnerSha256, 'unchanged Native execution runner');
    const runner = path.join(directory, esm?'check.mjs':'check.cjs');
    fs.writeFileSync(runner,source);
    const result = spawnSync(process.execPath,[runner],{cwd:directory,encoding:'utf8',timeout:30000});
    if (result.error) throw result.error;
    assert.equal(result.signal,null);
    const actual = {exit:result.status,stdout:result.stdout,stderr:result.stderr,runnerSha256:sha(source)};
    fs.writeFileSync(path.join(directory,'observation.json'),JSON.stringify(actual,null,2)+'\n');
    assert.deepEqual(actual,{exit:expected.exit,stdout:expected.stdout,stderr:expected.stderr,
        runnerSha256:expected.runnerSha256},wanted.id+': unchanged JavaScript execution');
    return {id:wanted.id,surface,...actual,products,unmodifiedProductJavaScript:true};
}

module.exports = {build, policyRefusal, consumer, runtime};
