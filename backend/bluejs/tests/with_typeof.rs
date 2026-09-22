// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `typeof identifier` inside a `with` block must resolve the identifier
//! against the with object first, and an unresolvable one is `"undefined"`
//! rather than a ReferenceError.
use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn truthy(source: &str) {
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
}

#[test]
fn typeof_reads_a_property_of_the_with_object() {
    truthy("var r;with({push:1}){r=typeof push}r==='number'");
    truthy("var r;with([1]){r=typeof push}r==='function'");
    truthy("var r;with({f(){}}){r=typeof f}r==='function'");
}

#[test]
fn typeof_of_an_unresolvable_name_inside_with_is_undefined() {
    truthy("var r;with({}){r=typeof neverDefinedAnywhere}r==='undefined'");
    truthy("var r;with({a:1}){r=typeof a+typeof b}r==='numberundefined'");
}

#[test]
fn typeof_falls_back_to_enclosing_bindings_and_globals() {
    truthy("var v='s';var r;with({}){r=typeof v}r==='string'");
    truthy("function f(){var local=1;var r;with({}){r=typeof local}return r}f()==='number'");
    truthy(
        "var r;with({}){r=typeof Array+typeof undefined+typeof Math}r==='functionundefinedobject'",
    );
}

#[test]
fn typeof_respects_symbol_unscopables() {
    truthy(
        "var x='outer';var r;\
         with({x:1,[Symbol.unscopables]:{x:true}}){r=typeof x}r==='string'",
    );
}

#[test]
fn typeof_reads_the_with_property_exactly_once() {
    truthy("var n=0;var r;with({get g(){n++;return 1}}){r=typeof g}n===1&&r==='number'");
}

#[test]
fn typeof_of_a_temporal_dead_zone_binding_still_throws() {
    truthy(
        "var threw=false;function f(){with({}){return typeof t}let t=1}\
         try{f()}catch(e){threw=e instanceof ReferenceError}threw",
    );
}

#[test]
fn typeof_outside_with_is_unchanged() {
    truthy("typeof neverDefinedAnywhere==='undefined'&&typeof Array==='function'");
    truthy("var v=1;typeof v==='number'");
}

#[test]
fn plain_reads_of_standard_globals_inside_with_resolve() {
    // Not only `typeof`: a bare `Math`, `Array` or `undefined` inside `with`
    // used to throw ReferenceError when nothing had touched globalThis yet.
    truthy("var r;with({}){r=Math}r===Math&&typeof Math.max==='function'");
    truthy("var r;with({}){r=Array}r===Array");
    truthy("var r;with({}){r=undefined}r===undefined");
    truthy("var r;with({a:1}){r=Object.keys({a})}r.length===1");
}
