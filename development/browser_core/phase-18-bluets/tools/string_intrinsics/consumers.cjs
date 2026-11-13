// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

function nativeDiagnostic(ts, diagnostic, root) {
    let file = null, position = null;
    if (diagnostic.file && diagnostic.start !== undefined) {
        const at = diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start);
        file = path.relative(root, diagnostic.file.fileName).replaceAll('\\', '/');
        position = {line: at.line + 1, column: at.character + 1, length: diagnostic.length};
    }
    return {
        code: diagnostic.code,
        message: Buffer.from(ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'), 'utf8').toString('utf8'),
        file, position,
        relatedInformation: (diagnostic.relatedInformation || []).map(row => nativeDiagnostic(ts, row, root)),
    };
}

function consumerVerifier(ts, implementation) {
    const cache = new Map();
    return (root, options, source, declarations) => {
        fs.writeFileSync(path.join(root, 'consumer.ts'), source);
        const host = ts.createCompilerHost(options), get = host.getSourceFile;
        host.getSourceFile = (file, version, ...rest) => {
            const relative = path.relative(path.dirname(implementation), file);
            if (relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) return get(file, version, ...rest);
            const key = file + ':' + version;
            if (!cache.has(key)) cache.set(key, get(file, version, ...rest));
            return cache.get(key);
        };
        const roots = ['consumer.ts', ...Object.keys(declarations)].sort();
        const program = ts.createProgram(roots.map(name => path.join(root, name)), {...options, noEmit: true}, host);
        const selected = program.getSourceFiles().map(file => path.relative(root, file.fileName).replaceAll('\\', '/'))
            .filter(name => !name.startsWith('../')).sort();
        assert.deepEqual(selected, roots, 'consumer must select only the emitted declarations');
        const diagnostics = ts.sortAndDeduplicateDiagnostics(ts.getPreEmitDiagnostics(program))
            .map(row => nativeDiagnostic(ts, row, root));
        return {accepts: diagnostics.length === 0, diagnostics};
    };
}

module.exports = consumerVerifier;
