// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const resolvePackageFile = require('./resolve_package.cjs');

module.exports = function loadTypeScript(command) {
    const ts = require(resolvePackageFile(command, 'typescript', 'lib/typescript.js'));
    if (ts.version !== '5.9.3') throw new Error(`Expected TypeScript 5.9.3, received ${ts.version}`);
    return ts;
};

module.exports.cliPath = function compilerCliPath(command) {
    return resolvePackageFile(command, 'typescript', 'bin/tsc');
};
