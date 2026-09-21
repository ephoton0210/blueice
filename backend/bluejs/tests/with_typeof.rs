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

#[test]
fn delete_of_a_name_removes_the_with_objects_property() {
    truthy("var o={p:1,q:2};var r;with(o){r=delete p}r===true&&!('p' in o)&&o.q===2");
    truthy("var o={};var r;with(o){r=delete missing}r===true");
    truthy(
        "var o=Object.defineProperty({},'p',{value:1});var r;with(o){r=delete p}r===false&&o.p===1",
    );
}

#[test]
fn delete_of_a_name_finds_the_innermost_with_object_first() {
    truthy("var a={p:1},b={p:2};with(a){with(b){delete p}}('p' in a)&&!('p' in b)");
    truthy("var a={p:1},b={};with(a){with(b){delete p}}!('p' in a)");
}

#[test]
fn delete_of_a_name_respects_unscopables_and_falls_back_to_bindings() {
    truthy(
        "var o={p:1,[Symbol.unscopables]:{p:true}};this.p=2;var r;with(o){r=delete p}r===true&&o.p===1&&!('p' in this)",
    );
    truthy("var v=1;var r;with({}){r=delete v}r===false&&v===1");
    truthy("function f(){var l=1;var r;with({}){r=delete l}return r===false&&l===1}f()");
    truthy("this.g=1;var r;with({}){r=delete g}r===true&&typeof g==='undefined'");
}

#[test]
fn delete_of_an_unqualified_global_name_deletes_the_global_property() {
    truthy("this.p=2;var r=delete p;r===true&&!('p' in this)");
    truthy("x=1;var r=delete x;r===true&&typeof x==='undefined'");
    truthy("var r=delete NaN;r===false");
    truthy("var v=1;var r=delete v;r===false&&v===1");
    truthy("let l=1;var r=delete l;r===false&&l===1");
    truthy("var r=delete neverDefined;r===true");
}
