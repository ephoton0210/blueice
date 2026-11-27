// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const base = require('../contextual_member_results/common.cjs');
const os = require('node:os');
const {fs, path, assert, sha, repo} = base;
const fixture = path.join(repo, 'backend/bluets/tests/fixtures/assertion_results');
const phase = path.join(repo, 'development/browser_core/phase-18-bluets');
const read = name => JSON.parse(fs.readFileSync(path.join(fixture, name)));

function projectCheckoutIoTool(contract, correction, actual) {
    const name = 'ci_inventory_checkout.test.cjs';
    assert.equal(correction.path, 'development/browser_core/phase-18-bluets/tools/' + name);
    assert.equal(correction.baseContractSha256, '962d5b8062eddf41fba8d5ee74c98b2ab80d9d5f8f87e59a4554161b9200e461');
    assert.equal(correction.kind, 'canonical-blob-streaming-host-correction');
    assert.equal(correction.beforeSha256, contract.preservation.originalToolHashes[name]);
    assert.equal(sha(correction.beforeText), correction.beforeSha256);
    for (const key of ['fixtureChanges','nativeExpectedOutputChanges','compilerChanges'])
        assert.deepEqual(correction[key], []);
    assert.equal(correction.regressionRequirementsUnchanged, true);
    const oldImport = "const { spawnSync, execFileSync } = require('node:child_process');";
    const newImport = "const { spawnSync } = require('node:child_process');";
    const oldRead = [
        "    const bytes = execFileSync('git', ['show', 'HEAD:' + file], {",
        '      cwd: root, maxBuffer: 32 * 1024 * 1024',
        '    });',
    ].join('\n');
    const newRead = [
        '    const captured = path.join(base, path.basename(file));',
        "    const descriptor = fs.openSync(captured, 'wx');",
        '    let result;',
        '    try {',
        "      result = spawnSync('git', ['show', 'HEAD:' + file], {",
        "        cwd: root, stdio: ['ignore', descriptor, 'pipe']",
        '      });',
        '    } finally { fs.closeSync(descriptor); }',
        '    assert.ifError(result.error);',
        '    assert.equal(result.status, 0, String(result.stderr));',
        '    const bytes = fs.readFileSync(captured);',
    ].join('\n');
    assert.equal(correction.beforeText.split(oldImport).length, 2);
    assert.equal(correction.beforeText.split(oldRead).length, 2);
    const expected = correction.beforeText.replace(oldImport,newImport).replace(oldRead,newRead);
    assert.equal(sha(expected), correction.afterSha256, 'only canonical blob I/O may change');
    assert(Buffer.from(actual).equals(Buffer.from(expected)), 'every repaired checkout-test byte');
    // Historical P0-J preservation still validates its original tool bytes;
    // this projection is permitted only after proving the exact newer I/O
    // implementation above. All real executions use the repaired file.
    return Buffer.from(correction.beforeText);
}

function validateOracleHostCorrections(contract, record, readTool) {
    const prefix = 'development/browser_core/phase-18-bluets/tools/';
    const names = ['record_gt4_overload_chains.cjs', 'record_gt8_return_context.cjs'];
    assert.equal(record.baseContractSha256, '962d5b8062eddf41fba8d5ee74c98b2ab80d9d5f8f87e59a4554161b9200e461');
    assert.equal(record.kind, 'mechanical-virtual-compiler-host-correction');
    assert.deepEqual(record.fixtureChanges, []);
    assert.deepEqual(record.nativeExpectedOutputChanges, []);
    assert.deepEqual(record.compilerChanges, []);
    assert.equal(record.regressionRequirementsUnchanged, true);
    assert.deepEqual(record.changes.map(row => row.path), names.map(name => prefix + name));
    const oldBlock = [
        '    const exists = host.fileExists, readFile = host.readFile, directoryExists = host.directoryExists;',
        '    host.fileExists = file => sources.has(file) || exists(file);',
        '    host.readFile = file => sources.get(file) ?? readFile(file);',
        '    host.directoryExists = file => file === base || directoryExists(file);',
        '    host.getSourceFile = (file, version, ...args) => {',
        '        if (sources.has(file)) return ts.createSourceFile(file, sources.get(file), version, true);',
        '        if (!libraries.has(file)) libraries.set(file, read(file, version, ...args));',
        '        return libraries.get(file);',
        '    };',
        '',
    ].join('\n');
    const newBlock = [
        '    host.getSourceFile = (file, version, ...args) => {',
        '        if (!libraries.has(file)) libraries.set(file, read(file, version, ...args));',
        '        return libraries.get(file);',
        '    };',
        "    require('./virtual_compiler_host.cjs')(ts, host, sources);",
        '',
    ].join('\n');
    const expected = {...contract.preservation.originalToolHashes};
    assert.equal(Object.keys(expected).length, 621);
    for (const [index, change] of record.changes.entries()) {
        const name = names[index];
        assert.equal(change.beforeSha256, expected[name], name + ': original baseline');
        assert.equal(sha(change.beforeText), change.beforeSha256, name + ': original bytes');
        assert.equal(change.beforeText.split(oldBlock).length, 2, name + ': exact single host block');
        const repaired = change.beforeText.replace(oldBlock, newBlock);
        assert.equal(sha(repaired), change.afterSha256, name + ': only the mechanical host correction');
        assert(Buffer.from(readTool(name)).equals(Buffer.from(repaired)), name + ': actual repaired bytes');
        expected[name] = change.afterSha256;
    }
    for (const [name, digest] of Object.entries(expected))
        assert.equal(sha(readTool(name)), digest, name + ': all original tools remain constrained');
    return expected;
}

function oracleHostCorrections(contract) {
    const bytes = fs.readFileSync(path.join(phase, 'ASSERTION_RESULTS_ORACLE_HOST_CORRECTIONS.json'));
    const digest = sha(bytes);
    assert.equal(digest, '287056fd44b301bdd1d58f165a381bda0bcbca59b4371004115b87232616f646');
    const checkoutPath = path.join(phase,'ERASED_TYPE_REGIONS_CHECKOUT_IO_CORRECTION.json');
    const checkout = fs.existsSync(checkoutPath) ? JSON.parse(fs.readFileSync(checkoutPath)) : null;
    validateOracleHostCorrections(contract, JSON.parse(bytes), name => {
        const actual = fs.readFileSync(path.join(phase,'tools',name));
        return checkout && name === 'ci_inventory_checkout.test.cjs'
            ? projectCheckoutIoTool(contract,checkout,actual) : actual;
    });
    return digest;
}

function policyAmendment(contractSha256) {
    const bytes = fs.readFileSync(path.join(phase, 'ASSERTION_RESULTS_POLICY_AMENDMENT.json'));
    const digest = sha(bytes);
    assert.equal(digest, 'e582d963cc10dc6b17fccdbcf355574610b35c81ff266c1cd0105241882fc9f1');
    const amendment = JSON.parse(bytes);
    assert.equal(amendment.baseContractSha256, contractSha256);
    assert.equal(amendment.userInstruction, '接受 unknown as T');
    assert.equal(amendment.preservationExceptionIsExhaustive, true);
    const directory = 'backend/bluets/tests/fixtures/mapped_inference/';
    assert.deepEqual(amendment.changes.map(row => row.path),
        [directory+'projection-owner-controls.json', directory+'FROZEN.json']);
    for (const change of amendment.changes) {
        assert.equal(sha(change.beforeText), change.beforeSha256);
        assert.equal(sha(fs.readFileSync(path.join(repo, change.path))), change.afterSha256);
    }
    const [control, frozen] = amendment.changes;
    assert.equal(control.beforeSha256, '8a1a81f5608ac08426df6579b2bc5b99bebffd22cdb8e2f1af1cd9f07fdca98d');
    const expected = JSON.parse(control.beforeText);
    const row = expected.cases.find(row => row.id === 'unknown-cannot-acquire-parameter');
    assert.equal(row.blueAccepts, false);
    assert.equal(row.accepts, true);
    row.blueAccepts = true;
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(repo, control.path))), expected,
        'only the explicitly approved pure-return verdict may change');
    const expectedFrozen = JSON.parse(frozen.beforeText);
    assert.equal(expectedFrozen.supplementarySha256['projection-owner-controls.json'], control.beforeSha256);
    expectedFrozen.supplementarySha256['projection-owner-controls.json'] = control.afterSha256;
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(repo, frozen.path))), expectedFrozen);
    return digest;
}

function authorities(ctx) {
    const frozen = read('FROZEN.json');
    for (const [name, digest] of Object.entries(frozen.sha256))
        assert.equal(sha(fs.readFileSync(path.join(fixture, name))), digest, name);
    assert.equal(sha(fs.readFileSync(ctx.implementation)), frozen.implementationSha256);
    for (const [name, digest] of Object.entries(frozen.libraries))
        assert.equal(sha(fs.readFileSync(path.join(path.dirname(ctx.implementation), name))), digest, name);
    const bytes = fs.readFileSync(path.join(phase, 'ASSERTION_RESULTS_CONTRACT.json'));
    assert.equal(sha(bytes), '962d5b8062eddf41fba8d5ee74c98b2ab80d9d5f8f87e59a4554161b9200e461');
    const contract = JSON.parse(bytes);
    assert.equal(sha(fs.readFileSync(path.join(fixture, 'FROZEN.json'))), contract.frozenFixtureSha256);
    const oracleHostCorrectionsSha256 = oracleHostCorrections(contract);
    const contractSha256 = sha(bytes);
    return {frozen, contract, contractSha256,
        policyAmendmentSha256:policyAmendment(contractSha256),oracleHostCorrectionsSha256};
}

function capture() {
    const explicit = process.env.BLUEICE_ASSERTION_RESULTS_OUTPUT;
    if (explicit) {
        const root = path.resolve(explicit);
        assert(!fs.existsSync(root), 'never overwrite an earlier assertion observation');
        fs.mkdirSync(root, {recursive:true});
        return root;
    }
    const parent = process.env.BLUEICE_ASSERTION_RESULTS_OBSERVATIONS ||
        process.env.BLUEICE_GE5_ROLE_REPORT_DIRECTORY || os.tmpdir();
    fs.mkdirSync(parent, {recursive:true});
    return fs.mkdtempSync(path.join(path.resolve(parent), 'bluets-assertion-results-'));
}

function completeMap(text, product, authored, name) {
    const decoded = require('../ge5_maps.cjs').decode(text, product);
    assert.deepEqual(decoded.coordinateIssues, [], 'generated map bounds: '+name);
    const lengths = decoded.metadata.sources.map(source => {
        const key = path.posix.normalize(path.posix.join('dist', path.posix.dirname(name),
            decoded.metadata.sourceRoot || '', source));
        assert(Object.hasOwn(authored, key), 'unselected map source: '+key);
        return authored[key].split(/\r\n|\r|\n/).map(line => line.length);
    });
    for (const segment of decoded.segments) {
        if (segment.length < 5) continue;
        const [,, index, line, column] = segment;
        assert(index >= 0 && index < lengths.length, 'map source index');
        assert(line >= 0 && line < lengths[index].length, 'map source line');
        assert(column >= 0 && column <= lengths[index][line], 'map UTF-16 source column');
    }
    return decoded;
}

function toolHashes() {
    return Object.fromEntries(['verify_assertion_results.cjs', 'assertion_results/common.cjs',
        'assertion_results/observations.cjs', 'assertion_results/audit.cjs',
        'assertion_results/policy.test.cjs'].map(name => [name,sha(fs.readFileSync(path.join(phase,'tools',name)))]));
}

module.exports = {...base, fixture, phase, read, authorities, capture, completeMap, toolHashes, validateOracleHostCorrections, projectCheckoutIoTool};
