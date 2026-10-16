// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

module.exports = function normalizeMapSource(source, directory, mapRoot) {
    const casePath = directory.replaceAll('\\', '/');
    const portableCasePath = casePath.replace(/^\/\/\?\//, '');
    let result = source.replace(casePath, '<case>')
        .replace(portableCasePath, '<case>')
        .replace(portableCasePath.replace(/^\//, ''), '<case>');
    // TypeScript uses a file URL for the same source on Windows when mapRoot
    // is rooted. The corpus records the corresponding Unix spelling.
    if (result.startsWith('file:///<case>/')) {
        if (mapRoot.startsWith('/') && !mapRoot.startsWith('//')) {
            result = result.replace('file:///<case>', '..<case>');
        } else if (mapRoot.includes('://')) {
            result = result.replace('file:///<case>', 'file://<case>');
        }
    }
    return result;
};
