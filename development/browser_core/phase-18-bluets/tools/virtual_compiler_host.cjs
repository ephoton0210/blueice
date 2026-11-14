// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Adapt only virtual filenames; source text, output bytes and disk fallbacks
// retain the pinned compiler's behavior and the host's case-sensitivity rule.
module.exports = function virtualCompilerHost(ts, host, entries) {
    const canonical = file => host.getCanonicalFileName(ts.normalizePath(file));
    const sources = new Map(), directories = new Set();
    for (const [file, text] of entries) {
        const normalized = ts.normalizePath(file), key = canonical(normalized);
        if (sources.has(key) && sources.get(key) !== text)
            throw Error('Conflicting virtual source: ' + file);
        sources.set(key, text);
        directories.add(canonical(ts.getDirectoryPath(normalized)));
    }
    const exists = host.fileExists.bind(host), readFile = host.readFile.bind(host);
    const directoryExists = host.directoryExists.bind(host), read = host.getSourceFile.bind(host);
    host.fileExists = file => sources.has(canonical(file)) || exists(file);
    host.readFile = file => sources.has(canonical(file)) ? sources.get(canonical(file)) : readFile(file);
    host.directoryExists = file => directories.has(canonical(file)) || directoryExists(file);
    host.getSourceFile = (file, version, ...args) => sources.has(canonical(file))
        ? ts.createSourceFile(file, sources.get(canonical(file)), version, true) : read(file, version, ...args);
    return host;
};
