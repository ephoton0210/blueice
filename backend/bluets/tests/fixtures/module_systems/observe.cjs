// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original, closed module loaders for K.7.2 runtime observations.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const {spawnSync} = require('child_process');

function ownedExtends(child, parent) {
    Object.setPrototypeOf(child, parent);
    child.prototype = Object.create(parent.prototype);
    Object.defineProperty(child.prototype, 'constructor', {
        value:child, writable:true, configurable:true,
    });
}

function dependency(file, name, root) {
    if (!name.startsWith('./') && !name.startsWith('../')) throw new Error(`Unexpected dependency: ${name}`);
    const target = path.resolve(path.dirname(file), name + '.js');
    if (!target.startsWith(root + path.sep)) throw new Error('Module escaped output root');
    return target;
}

function ordinary(entry, module, amdBranch, globalHelper, provider) {
    const root = path.dirname(entry);
    const modules = new Map();
    function load(file) {
        if (modules.has(file)) return modules.get(file);
        let exports = {};
        modules.set(file, exports);
        const require = name => name === 'tslib' ? provider : load(dependency(file, name, root));
        const context = globalHelper ? {__extends:ownedExtends} : {};
        if (module === 'AMD' || amdBranch) {
            context.define = (...args) => {
                const factory = args.pop(), dependencies = args.pop();
                const result = factory(...dependencies.map(name => name === 'exports' ? exports : name === 'require' ? require : require(name)));
                if (result !== undefined) {
                    exports = result;
                    modules.set(file, result);
                }
            };
            context.define.amd = {};
        } else {
            context.exports = exports;
            context.module = {exports};
            context.require = require;
        }
        vm.runInNewContext(fs.readFileSync(file, 'utf8'), context, {filename:file});
        if (module !== 'AMD' && !amdBranch) {
            exports = context.module.exports;
            modules.set(file, exports);
        }
        return exports;
    }
    return load(entry);
}

async function system(entry, globalHelper, provider) {
    const root = path.dirname(entry);
    const records = new Map();
    function register(file) {
        if (records.has(file)) return records.get(file);
        const record = {exports:{}, listeners:[], executed:false};
        records.set(file, record);
        const publish = (name, value) => {
            Object.assign(record.exports, typeof name === 'object' ? name : {[name]:value});
            for (const listener of record.listeners) listener(record.exports);
            return value;
        };
        const context = globalHelper ? {__extends:ownedExtends} : {};
        context.System = {register(dependencies, factory) {
            record.dependencies = dependencies;
            record.body = factory(publish, {id:file});
        }};
        vm.runInNewContext(fs.readFileSync(file, 'utf8'), context, {filename:file});
        record.imports = record.dependencies.map((name, index) => {
            const setter = record.body.setters[index];
            if (name === 'tslib') {
                if (setter) setter(provider);
                return null;
            }
            const imported = register(dependency(file, name, root));
            if (setter) {
                imported.listeners.push(setter);
                setter(imported.exports);
            }
            return imported;
        });
        return record;
    }
    async function execute(record) {
        if (record.executed) return;
        record.executed = true;
        for (const imported of record.imports) if (imported) await execute(imported);
        await record.body.execute();
    }
    const record = register(entry);
    await execute(record);
    return record.exports;
}

async function observe(directory, item) {
    const record = item.reference;
    const extension = item.entry.endsWith('.mts') ? '.mjs' : item.entry.endsWith('.cts') ? '.cjs' : '.js';
    const entry = path.join(directory, 'main' + extension);
    if (item.family === 'per-file') {
        const output = spawnSync(process.execPath, [entry], {encoding:'utf8'});
        if (output.status !== 0) throw new Error(output.stderr);
        return {stdout:output.stdout};
    }
    let provider;
    if (item.family === 'helper-options' && record.provider) {
        const exports = {};
        const file = path.join(path.dirname(directory), 'node_modules/tslib/index.js');
        vm.runInNewContext(fs.readFileSync(file, 'utf8'), {exports}, {filename:file});
        provider = exports;
    }
    const load = globalHelper => record.module === 'System'
        ? system(entry, globalHelper, provider)
        : ordinary(entry, record.module, false, globalHelper, provider);
    const namespace = await load(item.family === 'helper-options');
    if (item.family === 'module') {
        const observations = [{before:namespace.before, result:Array.from(namespace.result)}];
        if (record.module === 'UMD') {
            const amd = ordinary(entry, record.module, true, false, provider);
            observations.push({before:amd.before, result:Array.from(amd.result)});
        }
        return {observations};
    }
    if (item.family === 'helper-options') {
        const instance = new namespace.Derived();
        const observation = {result:namespace.result, instanceValue:instance.value,
            constructorIdentity:instance.constructor === namespace.Derived};
        let missingGlobal = null;
        try { await load(false); }
        catch (error) { missingGlobal = {name:error.name, message:error.message}; }
        return {observation, missingGlobal};
    }
    const observation = {result:namespace.result};
    if ('before' in namespace) observation.before = namespace.before;
    if ('default' in namespace) observation.default = typeof namespace.default === 'function'
        ? {name:namespace.default.name, value:namespace.default()} : namespace.default;
    return {observation};
}

module.exports = observe;
if (require.main === module) {
    observe(process.argv[2], JSON.parse(process.argv[3])).then(
        value => process.stdout.write(JSON.stringify(value) + '\n'),
        error => {console.error(error); process.exitCode = 1;},
    );
}
