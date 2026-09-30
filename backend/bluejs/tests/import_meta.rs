// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `import.meta` and `import()` are resolved against the *module a function
//! was declared in* (GetActiveScriptOrModule), not against whichever module
//! happens to be running when the function is called. Hoisted function
//! declarations are instantiated during module declaration instantiation,
//! before any module body runs, so that phase has to know its module too.

use blueice_bluejs::{compile_module, parse_module, Value, Vm};
use std::collections::HashMap;

fn run(sources: &[(&str, &str)]) -> Result<Value, blueice_bluejs::RuntimeError> {
    let modules: HashMap<_, _> = sources
        .iter()
        .map(|(name, source)| {
            (
                format!("t/{name}"),
                compile_module(&parse_module(source).unwrap_or_else(|e| panic!("{name}: {e:?}")))
                    .unwrap(),
            )
        })
        .collect();
    Vm::default().execute_module_graph("t/main.js", &modules)
}

#[test]
fn a_hoisted_function_declaration_reads_its_own_modules_import_meta() {
    let result = run(&[
        (
            "main.js",
            "import { meta, getMeta } from './dep.js';
             getMeta() === meta && import.meta !== meta && import.meta !== getMeta()",
        ),
        (
            "dep.js",
            "export var meta = import.meta; export function getMeta() { return import.meta; }",
        ),
    ]);
    assert_eq!(result, Ok(Value::Bool(true)));
}

#[test]
fn a_hoisted_function_called_from_another_module_resolves_import_against_its_own_module() {
    // `main.js` lives in `t/`, `sub/dep.js` in `t/sub/`; only the latter's own
    // referrer can resolve `./leaf.js` to `t/sub/leaf.js`.
    let result = run(&[
        (
            "main.js",
            "import { load } from './sub/dep.js';
             let out; load().then(ns => { out = ns.value; });
             await null; await null; await null; await null;
             out",
        ),
        (
            "sub/dep.js",
            "export function load() { return import('./leaf.js'); }",
        ),
        ("sub/leaf.js", "export var value = 7;"),
    ]);
    assert_eq!(result, Ok(Value::Number(7.0)));
}
