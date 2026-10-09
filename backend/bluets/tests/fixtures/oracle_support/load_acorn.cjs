// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs');
const path = require('path');

module.exports = function loadAcorn(command = process.env.BLUEICE_ACORN_ORACLE || 'acorn') {
    const candidates = path.isAbsolute(command) || /[/\\]/.test(command)
        ? [path.resolve(command)]
        : (process.env.PATH || '').split(path.delimiter).map(dir => path.join(dir, command));
    const executable = candidates.find(candidate => {
        try {
            fs.accessSync(candidate, fs.constants.X_OK);
            return fs.statSync(candidate).isFile();
        } catch { return false; }
    });
    if (!executable) throw new Error(`Cannot resolve pinned Acorn oracle: ${command}`);
    const acorn = require(path.resolve(fs.realpathSync(executable), '../../dist/acorn.js'));
    if (acorn.version !== '8.15.0') throw new Error(`Expected Acorn 8.15.0, received ${acorn.version}`);
    return acorn;
};
