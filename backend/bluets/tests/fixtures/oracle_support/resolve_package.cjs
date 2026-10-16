// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('node:fs');
const path = require('node:path');

function oracleExecutable(command) {
    const candidates = path.isAbsolute(command) || /[/\\]/.test(command)
        ? [path.resolve(command)]
        : (process.env.PATH || '').split(path.delimiter).map(dir => path.join(dir, command));
    const executable = candidates.find(candidate => {
        try {
            fs.accessSync(candidate, fs.constants.X_OK);
            return fs.statSync(candidate).isFile();
        } catch { return false; }
    });
    if (!executable) throw new Error(`Cannot resolve pinned oracle executable: ${command}`);
    return fs.realpathSync(executable);
}

module.exports = function resolvePackageFile(command, packageName, relativeFile) {
    const executable = oracleExecutable(command);
    const directory = path.dirname(executable);
    const candidates = [
        path.resolve(executable, '../..', relativeFile),
        path.resolve(directory, '..', packageName, relativeFile),
        path.resolve(directory, 'node_modules', packageName, relativeFile),
    ];
    const file = candidates.find(candidate => {
        try { return fs.statSync(candidate).isFile(); }
        catch { return false; }
    });
    if (!file) throw new Error(`Cannot resolve pinned ${packageName} package from ${executable}`);
    return file;
};
