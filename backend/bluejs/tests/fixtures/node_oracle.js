// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Use a fresh realm per fixture; the Rust harness likewise creates a new VM.
// Numeric comparisons use exact IEEE bits (with canonical NaN), never
// decimal formatting or approximate equality that could hide signed zero.
const fs = require('node:fs');
const vm = require('node:vm');
// One hex-encoded UTF-8 script per line, including multiline scripts.
for (const encoded of fs.readFileSync(0, 'utf8').split('\n')) {
    if (!encoded) continue;
    const source = Buffer.from(encoded, 'hex').toString('utf8');
    let result;
    try {
        const value = vm.runInNewContext(source, {}, {timeout: 1000});
        if (value === undefined) result = 'undefined';
        else if (value === null) result = 'null';
        else if (typeof value === 'number') {
            const bits = Buffer.alloc(8);
            bits.writeDoubleBE(value);
            result = Number.isNaN(value) ? 'number:NaN' : 'number:' + bits.toString('hex');
        } else if (typeof value === 'string') {
            // Exact UTF-16 code units: UTF-8 would replace lone surrogates.
            result = 'string:';
            for (let i = 0; i < value.length; i++) result += value.charCodeAt(i).toString(16).padStart(4, '0');
        }
        else if (typeof value === 'boolean') result = 'bool:' + value;
        else throw new Error('Fixture returned a non-primitive value');
    } catch (error) { result = 'error:' + error.name; }
    console.log(result);
}
