// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs');
const acorn = require('./load_acorn.cjs')();
const [file, target, moduleKind] = process.argv.slice(2);
const edition = target.toLowerCase() === 'esnext' ? 'latest'
    : target.toLowerCase() === 'es5' ? 5 : Number(target.slice(2));
if (edition !== 'latest' && edition !== 5 && !(edition >= 2015 && edition <= 2023)) {
    throw new Error(`Unsupported syntax target: ${target}`);
}
acorn.parse(fs.readFileSync(file, 'utf8'), {
    ecmaVersion: edition, sourceType: moduleKind === 'commonjs' ? 'script' : 'module',
});
