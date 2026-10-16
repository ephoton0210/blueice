// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const resolvePackageFile = require('./resolve_package.cjs');

module.exports = function loadAcorn(command = process.env.BLUEICE_ACORN_ORACLE || 'acorn') {
    const acorn = require(resolvePackageFile(command, 'acorn', 'dist/acorn.js'));
    if (acorn.version !== '8.15.0') throw new Error(`Expected Acorn 8.15.0, received ${acorn.version}`);
    return acorn;
};
