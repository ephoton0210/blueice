// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const path = require('path');

// TypeScript can expand a Windows 8.3 parent (RUNNER~1) while Node retains it
// in the temporary directory. Match the unique temporary fixture and case
// components when the native relative path does not identify an in-root file.
function relativeFixturePath(directory, filename) {
    const relative = path.relative(directory, filename).replaceAll('\\', '/');
    if (relative && relative !== '..' && !relative.startsWith('../')
        && !path.isAbsolute(relative) && !/^[A-Za-z]:\//.test(relative)) return relative;
    const marker = '/' + path.basename(path.dirname(directory)) + '/' + path.basename(directory) + '/';
    const candidate = filename.replaceAll('\\', '/');
    const offset = candidate.toLowerCase().lastIndexOf(marker.toLowerCase());
    return offset < 0 ? null : candidate.slice(offset + marker.length);
}

module.exports = relativeFixturePath;
