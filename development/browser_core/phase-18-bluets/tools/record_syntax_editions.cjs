// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independent syntax evidence for the edition-selecting BlueJS parser gate.
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const root = path.resolve(__dirname, '../../../..');
const fixtures = path.join(root, 'backend/bluets/tests/fixtures');
const acorn = require(path.join(fixtures, 'oracle_support/load_acorn.cjs'))();
const corpus = path.join(fixtures, 'target_syntax');
const settings = JSON.parse(fs.readFileSync(path.join(corpus, 'settings.json'), 'utf8'));
if (settings.version !== acorn.version) throw new Error('Unpinned syntax oracle');
const cases = [];
const rows = [];
for (const item of settings.cases) {
    const source = fs.readFileSync(path.join(corpus, item.entry), 'utf8').replaceAll('\r\n', '\n');
    const verdicts = {};
    for (const target of settings.targets) {
        const edition = target === 'ESNext' ? 'latest' : target === 'ES5' ? 5 : Number(target.slice(2));
        let accepts = true;
        try {
            acorn.parse(source, { ecmaVersion: edition, sourceType: item.sourceType });
        } catch (error) {
            if (!(error instanceof SyntaxError)) throw error;
            accepts = false;
        }
        const expected = edition === 'latest' || edition >= item.firstTarget;
        if (accepts !== expected) throw new Error(`${item.entry}/${target}: expected ${expected}, received ${accepts}`);
        verdicts[target] = accepts;
        rows.push(`${item.entry}\t${item.sourceType}\t${target}\t${accepts ? 'accept' : 'reject'}\n`);
    }
    cases.push({entry: item.entry, sourceType: item.sourceType,
        sourceSha256: crypto.createHash('sha256').update(source).digest('hex'), verdicts});
}
const record = JSON.stringify({ version: acorn.version, targets: settings.targets,
    specification: settings.specification, cases }, null, 2) + '\n';
const matrix = rows.join('');
for (const [name, text] of [['reference.json', record], ['edition-verdicts.tsv', matrix]]) {
    const file = path.join(corpus, name);
    if (process.env.BLUEICE_WRITE_SYNTAX_EDITIONS === '1') fs.writeFileSync(file, text);
    else if (fs.readFileSync(file, 'utf8').replaceAll('\r\n', '\n') !== text) {
        throw new Error(`Syntax edition evidence changed: ${name}`);
    }
}
process.stdout.write(JSON.stringify({ witnesses: cases.length, observations: rows.length,
    accepts: cases.reduce((sum, item) => sum + Object.values(item.verdicts).filter(Boolean).length, 0),
    rejects: cases.reduce((sum, item) => sum + Object.values(item.verdicts).filter(value => !value).length, 0) }) + '\n');
