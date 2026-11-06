// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Exact execution framing retained from the frozen Native-v4 observations.
function executionScript(esm) {
    const load = esm ? 'import(require("node:url").pathToFileURL(process.argv[1]).href)' : 'require(process.argv[1])';
    return 'Promise.resolve().then(()=>' + load + ').then(module=>Promise.resolve(module.result)).then(value=>{if(value!==undefined)process.stdout.write(JSON.stringify(value));}).catch(error=>{const observed={name:error&&error.name!==undefined?String(error.name):"Error",code:error&&error.code!==undefined?error.code:null,message:error&&error.message!==undefined?String(error.message):String(error)};process.stderr.write(JSON.stringify(observed)+"\\n");process.exitCode=1;});';
}
const syntaxProjection = rows => [...rows].sort((a, b) => a.file.localeCompare(b.file));
const preservedProjection = value => value ? {...value, syntax: syntaxProjection(value.syntax)} : null;
function diagnosticProjection(value, nested = false) {
    return {code: value.code, ...!nested ? {category: value.category} : {}, file: value.file,
        start: value.start, length: value.length, message: value.message,
        related: (value.related || []).map(item => diagnosticProjection(item, true))};
}
const outcome = value => value ? {exit: value.exit, signal: value.signal, error: value.error,
    stdout: value.stdout, stderr: value.stderr} : null;
module.exports = {executionScript, syntaxProjection, preservedProjection, diagnosticProjection, outcome};
