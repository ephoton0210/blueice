// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs'), path = require('path'), vm = require('vm'), assert = require('assert');
const ts = require('../oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE || 'tsc');
const acorn = require('../oracle_support/load_acorn.cjs')();
const [file, target, moduleKind, id] = process.argv.slice(2);
const javascript = fs.readFileSync(file, 'utf8');

async function execute(text) {
    let exported;
    if (moduleKind === 'CommonJS') {
        const module = {exports: {}};
        vm.runInNewContext(text, {module, exports: module.exports}, {filename: id});
        exported = module.exports;
    } else {
        exported = await import('data:text/javascript;base64,' + Buffer.from(text + '\n//# sourceURL=' + id).toString('base64'));
    }
    return JSON.parse(JSON.stringify(exported.result));
}

async function main() {
    const edition = target === 'ESNext' ? 'latest' : target === 'ES5' ? 5 : Number(target.slice(2));
    let syntaxAccepted = true;
    try { acorn.parse(javascript, {ecmaVersion: edition, sourceType: moduleKind === 'CommonJS' ? 'script' : 'module'}); }
    catch { syntaxAccepted = false; }
    const parsed = ts.createSourceFile(path.basename(file), javascript, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
    const typescriptParseErrors = parsed.parseDiagnostics.map(d => ({code: d.code, start: d.start, length: d.length, message: ts.flattenDiagnosticMessageText(d.messageText, '\n')}));
    const proposalFeatures = {decorators: 0, autoAccessors: 0};
    function visit(node) {
        if (ts.canHaveDecorators(node)) proposalFeatures.decorators += (ts.getDecorators(node) || []).length;
        if (ts.isPropertyDeclaration(node) && node.modifiers?.some(m => m.kind === ts.SyntaxKind.AccessorKeyword)) proposalFeatures.autoAccessors++;
        ts.forEachChild(node, visit);
    }
    visit(parsed);
    let observation = null, runtimeErrorCategory = null, proposalExecution = null;
    try { observation = await execute(javascript); }
    catch (error) { runtimeErrorCategory = error.name; }
    if (runtimeErrorCategory && target === 'ESNext' && (proposalFeatures.decorators || proposalFeatures.autoAccessors)) {
        assert.deepStrictEqual(typescriptParseErrors, []);
        const lowered = ts.transpileModule(javascript, {fileName: 'main.js', reportDiagnostics: true,
            compilerOptions: {target: ts.ScriptTarget.ES2023, module: moduleKind === 'CommonJS' ? ts.ModuleKind.CommonJS : ts.ModuleKind.ESNext,
                newLine: ts.NewLineKind.LineFeed, experimentalDecorators: false}});
        assert.deepStrictEqual(lowered.diagnostics, []);
        proposalExecution = {adapter: 'typescript@5.9.3 emitted-JavaScript-only ES2023 lowering', observation: await execute(lowered.outputText)};
    }
    console.log(JSON.stringify({syntaxAccepted, typescriptParseErrors, proposalFeatures, observation, runtimeErrorCategory, proposalExecution}));
}
main().catch(error => {console.error(error); process.exitCode = 1;});
