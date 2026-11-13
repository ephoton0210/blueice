// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

function inventory(root, relative = '') {
    if (!fs.existsSync(root)) return {};
    const result = {};
    for (const entry of fs.readdirSync(path.join(root, relative), {withFileTypes: true})) {
        const name = relative ? relative + '/' + entry.name : entry.name;
        if (entry.isDirectory()) Object.assign(result, inventory(root, name));
        else if (entry.isFile()) result[name] = fs.readFileSync(path.join(root, name), 'utf8');
        else throw Error('unexpected non-regular compiler output: ' + name);
    }
    return result;
}

function writeSources(root, entry) {
    fs.writeFileSync(path.join(root, 'main.ts'), entry.source);
    for (const [name, text] of Object.entries(entry.files || {})) {
        assert(!path.isAbsolute(name) && !name.split(/[\\/]/).includes('..'));
        const file = path.join(root, name);
        fs.mkdirSync(path.dirname(file), {recursive: true});
        fs.writeFileSync(file, text);
    }
}

function blueDiagnostic(row) {
    const value = row.typescript ?? row;
    assert(value && typeof value.code === 'number', 'diagnostic must have a Native counterpart');
    return {
        code: value.code, message: value.message,
        file: value.span?.module ?? null, position: value.position ?? null,
        relatedInformation: (value.relatedInformation || []).map(blueDiagnostic),
    };
}

module.exports = {inventory, writeSources, blueDiagnostic};
