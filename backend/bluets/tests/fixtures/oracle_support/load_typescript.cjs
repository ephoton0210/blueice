// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs');
const path = require('path');

module.exports = function loadTypeScript(command) {
    const candidates = path.isAbsolute(command) || /[/\\]/.test(command)
        ? [path.resolve(command)]
        : (process.env.PATH || '').split(path.delimiter).map(dir => path.join(dir, command));
    const executable = candidates.find(candidate => {
        try {
            fs.accessSync(candidate, fs.constants.X_OK);
            return fs.statSync(candidate).isFile();
        } catch {
            return false;
        }
    });
    if (!executable) throw new Error(`Cannot resolve TypeScript oracle: ${command}`);
    const ts = require(path.resolve(fs.realpathSync(executable), '../../lib/typescript.js'));
    if (ts.version !== '5.9.3') throw new Error(`Expected TypeScript 5.9.3, received ${ts.version}`);
    return ts;
};
