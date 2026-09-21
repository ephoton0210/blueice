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

#[test]
fn a_var_initializer_inside_with_assigns_through_the_with_object() {
    // §14.3.2.1: the declared name is resolved as a reference before the
    // initializer runs, so a with object that has the property receives it.
    truthy("var o={v:'a'};with(o){var v='b'}o.v==='b'&&v===undefined");
    truthy("var o={};with(o){var w='b'}!('w' in o)&&w==='b'");
    truthy("var o={v:'a'};with(o){var v}o.v==='a'&&v===undefined");
    truthy(
        "var o={x:1,y:2};with(o){var x=10,y=20}o.x===10&&o.y===20&&x===undefined&&y===undefined",
    );
    truthy("var o={v:1};with(o){var f=function(){}}f.name==='f'&&!('f' in o)");
    truthy("var o={f:0};with(o){var f=function(){}}o.f.name==='f'");
    // The reference is resolved before the initializer can add the property.
    truthy("var o={};with(o){var z=(o.z='inner',1)}o.z==='inner'&&z===1");
}

#[test]
fn the_completion_value_of_with_is_the_body_value_or_undefined() {
    // §14.11.2: UpdateEmpty(C, undefined), so an empty body leaves undefined
    // even after an earlier statement produced a value.
    truthy("eval('1; with({}) { }') === undefined");
    truthy("eval('2; with({}) { 3; }') === 3");
    truthy("eval('1; do { 2; with({}) { 3; break; } 4; } while (false);') === 3");
    truthy("eval('5; do { 6; with({}) { break; } 7; } while (false);') === undefined");
    truthy("eval('8; do { 9; with({}) { 10; continue; } 11; } while (false)') === 10");
    truthy("eval('12; do { 13; with({}) { continue; } 14; } while (false)') === undefined");
}

#[test]
fn object_environment_reads_and_writes_probe_the_binding_again() {
    // HasBinding (has + @@unscopables) resolves the name; GetBindingValue and
    // SetMutableBinding then run HasProperty once more before Get/Set.
    let trace = |body: &str| -> String {
        let source = format!(
            "var log = []; var env = {{ p: 0 }};
             var proxy = new Proxy(env, {{
               has(t, k) {{ log.push('has:' + String(k)); return Reflect.has(t, k); }},
               get(t, k, r) {{ log.push('get:' + String(k)); return Reflect.get(t, k, r); }},
               set(t, k, v, r) {{ log.push('set:' + String(k)); return Reflect.set(t, k, v, r); }},
             }});
             with (proxy) {{ {body} }}
             log.join()"
        );
        match evaluate(&source).unwrap() {
            Value::String(text) => text.to_utf8().unwrap(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(
        trace("p;"),
        "has:p,get:Symbol(Symbol.unscopables),has:p,get:p"
    );
    assert_eq!(
        trace("p = 1;"),
        "has:p,get:Symbol(Symbol.unscopables),has:p,set:p"
    );
    assert_eq!(
        trace("p += 1;"),
        "has:p,get:Symbol(Symbol.unscopables),has:p,get:p,has:p,set:p"
    );
}

#[test]
fn a_binding_that_vanishes_during_resolution_reads_as_undefined_or_throws_when_strict() {
    // The @@unscopables getter deletes the property after HasBinding saw it.
    truthy(
        "var env = { p: 1, get [Symbol.unscopables]() { delete env.p; return {}; } };
         var r; with (env) { r = p; } r === undefined",
    );
    truthy(
        "var env = { p: 1, get [Symbol.unscopables]() { delete env.p; return {}; } };
         var r; with (env) { r = (function() { 'use strict'; try { return p; } catch (e) { return e instanceof ReferenceError; } })(); } r === true",
    );
}
