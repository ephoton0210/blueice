// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A function created inside a `with` statement closes over the with
//! object's Environment Record (§10.2.1 [[Environment]]): its free names
//! resolve through it whenever it is called, not through whichever `with`
//! happens to be active at the call site. Every script also runs under a
//! one-object nursery, where each allocation may collect.
use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn evaluate(source: &str, nursery_capacity: Option<usize>) -> Result<Value, String> {
    let mut config = VmConfig::default();
    if let Some(capacity) = nursery_capacity {
        config.heap.nursery_capacity = capacity;
    }
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .map_err(|error| format!("{error:?}"))
}

fn truthy(source: &str) {
    assert_eq!(evaluate(source, None), Ok(Value::Bool(true)), "{source}");
    assert_eq!(
        evaluate(source, Some(1)),
        Ok(Value::Bool(true)),
        "GC-stress mode: {source}"
    );
}

#[test]
fn a_function_created_in_with_reads_through_the_object_after_the_with_ends() {
    truthy("var o={v:1};var f;with(o){f=function(){return v}}o.v=2;f()===2");
    truthy("var o={v:1};var f;with(o){f=()=>v}o.v=5;f()===5");
    truthy("var o={v:1};var f;with(o){f=function(){v=3}}f();o.v===3");
    truthy("var o={};var f;with(o){f=function(){return typeof v}}var a=f();o.v=1;a==='undefined'&&f()==='number'");
}

#[test]
fn a_function_created_outside_with_does_not_see_the_active_with_object() {
    truthy("var v='outer';function g(){return v}var r;with({v:'inner'}){r=g()}r==='outer'");
    truthy("var v='outer';var r;with({v:'inner'}){r=(function(){var f=function(){return v};return f()})()}r==='inner'");
}

#[test]
fn nested_with_objects_resolve_innermost_first_from_a_closure() {
    truthy(
        "var a={p:'a',q:'a'},b={p:'b'};var f;with(a){with(b){f=function(){return p+q}}}f()==='ba'",
    );
    truthy("var a={p:1},b={p:2};var f;with(a){with(b){f=function(){return delete p}}}f()===true&&!('p' in b)&&a.p===1");
}

#[test]
fn a_closure_still_sees_its_own_and_enclosing_bindings_first() {
    truthy("var o={x:'obj'};var f;with(o){f=function(x){return x}}f('param')==='param'");
    truthy("var o={x:'obj'};var f;with(o){f=function(){var x='local';return x}}f()==='local'");
    truthy("var o={y:'obj'};var x='fn';var f;with(o){f=function(){return x}}f()==='fn'");
    truthy("var o={};var f;with(o){f=function(){return Math.max(1,2)}}f()===2");
}

#[test]
fn unscopables_and_direct_eval_apply_inside_a_closure() {
    truthy("var v='global';this.v=v;var o={v:'obj',[Symbol.unscopables]:{v:true}};var f;with(o){f=function(){return v}}f()==='global'");
    truthy("var o={v:7};var f;with(o){f=function(){return eval('v')}}f()===7");
    truthy("var o={};var f;with(o){f=function(){eval('var w=1');return typeof w}}f()==='number'&&!('w' in o)");
}

#[test]
fn update_and_compound_assignment_resolve_the_reference_before_reading() {
    // `x++` reads through a getter that deletes the property; the write goes
    // to the with object the name resolved to, recreating it.
    truthy(
        "var scope={get x(){delete this.x;return 2}};var x=0;with(scope){(function(){x++})()}scope.x===3&&x===0",
    );
    truthy(
        "var scope={get x(){delete this.x;return 2}};var x=0;with(scope){x++}scope.x===3&&x===0",
    );
    truthy(
        "var scope={get x(){delete this.x;return 2}};var n=0;var r;with(scope){(function(){'use strict';try{n++;x+=1;n++}catch(e){r=e instanceof ReferenceError}})()}r===true&&n===1&&!('x' in scope)",
    );
}

#[test]
fn a_callee_that_returns_restores_the_callers_with_objects() {
    truthy("var o={v:'o'};var p={v:'p'};var f;with(p){f=function(){return v}}var r;with(o){var a=f();r=v+a}r==='op'");
    truthy("var o={v:1};var f;with(o){f=function(){return v}}var r;with(o){f();r=v}r===1");
}

#[test]
fn a_function_called_by_name_inside_with_gets_the_with_object_as_this() {
    truthy("var r;var o={m(){r=this}};with(o){m()}r===o");
    truthy("var o={get g(){return this}};var r;with(o){r=g}r===o");
    // A name found outside the with objects keeps an undefined receiver.
    truthy("var r='unset';function f(){'use strict';r=this}with({}){f()}r===undefined");
    // Through a closure created in the with statement, too.
    truthy("var r;var o={m(){r=this}};var f;with(o){f=function(){m()}}f();r===o");
    // Arguments are still evaluated after the callee, and calls still work.
    truthy("var o={sum(a,b){return a+b}};var r;with(o){r=sum(1,2)}r===3");
    truthy("var x=(function(){return 5});var r;with({}){r=x()}r===5");
}

#[test]
fn a_generator_resumes_with_its_own_with_objects_and_leaks_none() {
    // `with` inside a generator body: the object stays in scope across yields
    // for the generator only, whoever resumes it.
    truthy(
        "var o = { a: 5 }; function* g() { with (o) { yield 0; return a; } }
         var it = g(); it.next(); var r; with ({ a: 99 }) { r = it.next().value; } r === 5",
    );
    truthy(
        "var seen = []; function* g() { with ({ v: 1 }) { yield v; seen.push(typeof v); yield v + 1; } }
         var it = g(); var a = it.next().value; var b = it.next().value;
         a === 1 && b === 2 && typeof v === 'undefined'",
    );
    // A generator created inside `with` resolves the object when it runs.
    truthy(
        "var o = { v: 'obj' }; var g; with (o) { g = function* () { yield v; }; }
         g().next().value === 'obj'",
    );
    truthy(
        "var o = { v: 'param' }; var g; with (o) { g = function* (x = v) { yield x; }; }
         g().next().value === 'param'",
    );
}
