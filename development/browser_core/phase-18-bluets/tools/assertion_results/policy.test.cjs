// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test = require('node:test'),assert = require('node:assert/strict');
const {policyRefusal} = require('./observations.cjs');
const site = {file:'main.ts',startByte:100,endByte:117};
const opaque = {severity:'error',btsCode:'BTS3003',typescript:null,
    rawMessage:'cannot prove a write through an unmodeled receiver avoids readonly members',
    span:{module:'memory:///main.ts',start:100,end:118}};

test('readonly and opaque policy errors must concern the actual authored mutation', () => {
    assert(policyRefusal([opaque],site,''));
    assert(policyRefusal([{...opaque,typescript:{code:2540}}],site,''));
    assert(!policyRefusal([],site,''));
    assert(!policyRefusal([{...opaque,severity:'warning'}],site,''));
    assert(!policyRefusal([{...opaque,span:{...opaque.span,start:0,end:50}}],site,''));
    assert(!policyRefusal([{...opaque,span:{...opaque.span,module:'memory:///foreign.ts'}}],site,''));
});

test('unrelated return errors and mixed errors cannot satisfy a mutation refusal', () => {
    const returned = {...opaque,typescript:{code:2322},span:{...opaque.span,start:0,end:200}};
    assert(!policyRefusal([returned],site,''));
    assert(!policyRefusal([opaque,returned],site,''));
    assert(!policyRefusal([{...opaque,btsCode:'BTS3004'}],site,''));
});

test('astral/CRLF prefixes shift authored byte spans by UTF-8 bytes', () => {
    const prefix = '// Unicode 😀\r\n';
    const offset = Buffer.byteLength(prefix);
    assert(policyRefusal([{...opaque,span:{...opaque.span,start:100+offset,end:118+offset}}],site,prefix));
    assert(!policyRefusal([opaque],site,prefix+' '.repeat(30)));
});

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {validateOracleHostCorrections: validate,projectCheckoutIoTool} = require('./common.cjs');
const repo = process.cwd();
const phase = path.join(repo, 'development/browser_core/phase-18-bluets');
const contract = JSON.parse(fs.readFileSync(path.join(phase, 'ASSERTION_RESULTS_CONTRACT.json')));
const record = JSON.parse(fs.readFileSync(path.join(phase, 'ASSERTION_RESULTS_ORACLE_HOST_CORRECTIONS.json')));
const tools = new Map(Object.keys(contract.preservation.originalToolHashes).map(name =>
    [name, fs.readFileSync(path.join(phase, 'tools', name))]));
const copy = value => JSON.parse(JSON.stringify(value));
const checkout = JSON.parse(fs.readFileSync(path.join(phase,'ERASED_TYPE_REGIONS_CHECKOUT_IO_CORRECTION.json')));
const check = (metadata = record, actual = tools) => validate(contract, metadata, name =>
    name === 'ci_inventory_checkout.test.cjs' ? projectCheckoutIoTool(contract,checkout,actual.get(name)) : actual.get(name));
const sha = value => crypto.createHash('sha256').update(value).digest('hex');

test('accept exactly two derived repairs and retain constraints on all 621 originals', () => {
    const actual = check();
    assert.equal(Object.keys(actual).length, 621);
    assert.deepEqual(Object.keys(actual).filter(name => actual[name] !== contract.preservation.originalToolHashes[name]),
        ['record_gt4_overload_chains.cjs', 'record_gt8_return_context.cjs']);
});
test('reject omitted, duplicate and additional exceptions', () => {
    for (const changes of [[], [record.changes[0]], [...record.changes, record.changes[0]],
        [record.changes[0], record.changes[0]]])
        assert.throws(() => check({...record, changes}));
});
test('reject a fixture or unrelated tool disguised as an allowed path', () => {
    for (const file of ['backend/bluets/tests/fixtures/gt4_overload_chains/reference.json',
        'development/browser_core/phase-18-bluets/tools/virtual_compiler_host.cjs']) {
        const altered = copy(record); altered.changes[0].path = file;
        assert.throws(() => check(altered));
    }
});
test('reject additional source edits even with an updated replacement digest', () => {
    for (let index = 0; index < 2; index++) {
        const altered = copy(record), actual = new Map(tools);
        const name = path.basename(altered.changes[index].path);
        const extra = Buffer.concat([actual.get(name), Buffer.from('// extra edit\n')]);
        actual.set(name, extra); altered.changes[index].afterSha256 = sha(extra);
        assert.throws(() => check(altered, actual));
    }
});
test('reject a modified original before-text or baseline digest', () => {
    const altered = copy(record); altered.changes[0].beforeText += '\n';
    assert.throws(() => check(altered));
    altered.changes[0].beforeSha256 = sha(altered.changes[0].beforeText);
    assert.throws(() => check(altered));
});
test('reject either recorder left unpatched', () => {
    for (const change of record.changes) {
        const actual = new Map(tools); actual.set(path.basename(change.path), Buffer.from(change.beforeText));
        assert.throws(() => check(record, actual));
    }
});
test('reject edits to the shared host or another original tool', () => {
    for (const name of ['virtual_compiler_host.cjs', 'account_inventory_rows.py']) {
        const actual = new Map(tools);
        actual.set(name, Buffer.concat([actual.get(name), Buffer.from('\n')]));
        assert.throws(() => check(record, actual));
    }
});
test('reject declarations of fixture, expectation, compiler or regression changes', () => {
    for (const key of ['fixtureChanges', 'nativeExpectedOutputChanges', 'compilerChanges']) {
        const altered = copy(record); altered[key] = ['unexpected'];
        assert.throws(() => check(altered));
    }
    assert.throws(() => check({...record, regressionRequirementsUnchanged:false}));
});

test('checkout I/O repair retains the exact original canonical-byte test after verified projection', () => {
    const original = projectCheckoutIoTool(contract,checkout,tools.get('ci_inventory_checkout.test.cjs'));
    assert.equal(sha(original),contract.preservation.originalToolHashes['ci_inventory_checkout.test.cjs']);
    assert.equal(sha(tools.get('ci_inventory_checkout.test.cjs')),checkout.afterSha256);
});

test('checkout I/O repair rejects an extra edit even with a replaced digest', () => {
    const extra = Buffer.concat([tools.get('ci_inventory_checkout.test.cjs'),Buffer.from('\n// extra\n')]);
    assert.throws(()=>projectCheckoutIoTool(contract,{...checkout,afterSha256:sha(extra)},extra));
});

test('checkout I/O repair refuses foreign paths, original bytes and semantic exceptions', () => {
    const actual = tools.get('ci_inventory_checkout.test.cjs');
    for (const edit of [{path:'foreign.cjs'},{beforeText:checkout.beforeText+'\n'},
        {fixtureChanges:['unexpected']},{nativeExpectedOutputChanges:['unexpected']},
        {compilerChanges:['unexpected']},{regressionRequirementsUnchanged:false}])
        assert.throws(()=>projectCheckoutIoTool(contract,{...checkout,...edit},actual));
});
