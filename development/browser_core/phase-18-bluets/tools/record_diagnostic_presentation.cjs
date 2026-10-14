// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Actual pinned CLI output; variable resource measurements are schema-checked.
const fs = require('fs');
const os = require('os');
const path = require('path');
const {spawnSync} = require('child_process');
const fixtures = path.resolve(__dirname, '../../../../backend/bluets/tests/fixtures');
const load = require(path.join(fixtures, 'oracle_support/load_typescript.cjs'));
const executable = process.env.BLUEICE_BLUETSC_ORACLE || 'tsc';
const ts = load(executable);
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'bluets-presentation-oracle-'));
const good = 'export const value: number = 1;\nconsole.log(value);\n';
const bad = 'export const value: number = "wrong";\nconsole.log(value);\n';
const related = 'class Child extends Base {}\nclass Base {}\nexport {};\n';
const observations = [
    ['valid-emit', good, {}],
    ['valid-noemit', good, {noEmit:true}],
    ['error-emit', bad, {}],
    ['error-noemit', bad, {noEmit:true}],
    ['error-blocked', bad, {noEmitOnError:true}],
    ['error-noemit-blocked', bad, {noEmit:true,noEmitOnError:true}],
    ['pretty-error', bad, {noEmit:true,pretty:true}],
    ['pretty-related', related, {noEmit:true,pretty:true}],
    ['plain-related', related, {noEmit:true}],
    ['pretty-multiple', 'export const first: number = "x";\nexport const second: boolean = 1;\n', {noEmit:true,pretty:true}],
    ['pretty-crlf', bad.replaceAll('\n','\r\n'), {noEmit:true,pretty:true}],
    ['summary-valid', good, {noEmit:true,diagnostics:true}],
    ['summary-error', bad, {noEmit:true,diagnostics:true}],
    ['cli-unknown', good, {}, ['--unknownPresentationOption']],
    ['config-unknown', good, {unknownPresentationOption:true,pretty:true,noEmitOnError:true}],
    ['pretty-tabs', '\texport const value: number = "wrong";\n', {noEmit:true,pretty:true}],
    ['pretty-unicode', '/* 😀 */ export const value: number = "wrong";\n', {noEmit:true,pretty:true}],
    ['pretty-multiline', 'declare function take(value: number): void;\ntake(\n    () => {\n        return 1;\n    }\n);\n', {noEmit:true,pretty:true}],
    ['pretty-library-related', 'const label: string = "hi"; label.toUppercase();\n', {noEmit:true,pretty:true}],
    ['pretty-multi-file', 'import { value } from "./dep.ts";\nconst other: number = "wrong";\nconsole.log(value, other);\n', {noEmit:true,pretty:true,allowImportingTsExtensions:true}, [], {'dep.ts':'export const value: number = "wrong";\nexport const second: boolean = 1;\n'}],
    ['pretty-long-span', 'declare function take(value: string): void;\ntake({\n    value: 1,\n    other: 2,\n    last: 3,\n    extra: 4,\n    final: 5\n});\n', {noEmit:true,pretty:true}],
    ['pretty-parser-error', 'export const value = "unterminated;\n', {noEmit:true,pretty:true}],
];
function files(root, current = root) {
    if (!fs.existsSync(current)) return [];
    return fs.readdirSync(current, {withFileTypes:true}).flatMap(item => item.isDirectory()
        ? files(root,path.join(current,item.name))
        : [path.relative(root,path.join(current,item.name)).split(path.sep).join('/')]);
}
try {
    const cases = observations.map(([name, source, options, extra = [],extraSources={}], index) => {
        const cwd = path.join(directory,String(index)); fs.mkdirSync(cwd);
        const config = {compilerOptions:{target:'es2022',module:'es2022',strict:true,outDir:'out',pretty:false,...options},files:['main.ts']};
        fs.writeFileSync(path.join(cwd,'main.ts'),source);
        for (const [name,text] of Object.entries(extraSources)) fs.writeFileSync(path.join(cwd,name),text);
        const configText=JSON.stringify(config,null,2)+'\n';
        fs.writeFileSync(path.join(cwd,'tsconfig.json'),configText);
        const args = ['--project','.',...extra];
        const result = spawnSync(executable,args,{cwd,encoding:'utf8',env:{...process.env,FORCE_COLOR:'0'}});
        if (result.error) throw result.error;
        const library=path.dirname(ts.getDefaultLibFilePath(config.compilerOptions));
        const output = result.stdout.replaceAll('\\','/').replace(
            /\x1b\[96m([^\x1b]+lib\.[^/\x1b]+\.d\.ts)\x1b\[0m/g,
            (whole,file) => fs.realpathSync(path.resolve(cwd,file))
                === fs.realpathSync(path.join(library,path.basename(file)))
                ? `\x1b[96m<typescript-lib>/${path.basename(file)}\x1b[0m` : whole,
        ).replaceAll(path.relative(cwd,library).split(path.sep).join('/'),'<typescript-lib>')
            .replaceAll(library.split(path.sep).join('/'),'<typescript-lib>')
            .replaceAll(cwd,'<project>');
        const summaryFields = [];
        const stdout = options.diagnostics ? output.split('\n').filter(line => {
            const match = line.match(/^([A-Za-z /]+):\s+([0-9.]+)(K|s)?$/);
            if (!match) return true;
            summaryFields.push({name:match[1],unit:match[3] || 'count'}); return false;
        }).join('\n') : output;
        const assets = files(path.join(cwd,'out')).sort();
        let runtime = null;
        if (assets.includes('main.js')) {
            const run = spawnSync('node',['--input-type=module','--eval',fs.readFileSync(path.join(cwd,'out/main.js'),'utf8')],{encoding:'utf8'});
            if (run.status !== 0) throw new Error(run.stderr); runtime = run.stdout;
        }
        return {name,source,extraSources,config,configText,args,exit:result.status,stdout,stderr:result.stderr,summaryFields,assets,runtime};
    });
    const record = {version:ts.version,exitCodes:{success:ts.ExitStatus.Success,outputsSkipped:ts.ExitStatus.DiagnosticsPresent_OutputsSkipped,outputsGenerated:ts.ExitStatus.DiagnosticsPresent_OutputsGenerated},cases};
    const text = JSON.stringify(record,null,2)+'\n';
    const matrix = cases.map(item => `${item.name}\t${item.exit === 0 ? 'accept' : 'reject'}\n`).join('');
    const destination = path.join(fixtures,'diagnostics/presentation-reference.json');
    const matrixPath = path.join(fixtures,'diagnostics/presentation-checker-matrix.tsv');
    if (process.env.BLUEICE_WRITE_PRESENTATION_MATRIX === '1') {
        fs.writeFileSync(destination,text); fs.writeFileSync(matrixPath,matrix);
    } else if (fs.readFileSync(destination,'utf8').replaceAll('\r\n','\n') !== text
        || fs.readFileSync(matrixPath,'utf8').replaceAll('\r\n','\n') !== matrix) throw new Error('Presentation observations differ from pinned TypeScript');
    process.stdout.write(JSON.stringify({cases:cases.length,exitCodes:record.exitCodes})+'\n');
} finally { fs.rmSync(directory,{recursive:true,force:true}); }
