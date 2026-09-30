// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Array.from` with a custom `this` constructor, through the real
//! parse/compile/execute path.
use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn evaluate_gc_stress(source: &str) -> Result<Value, RuntimeError> {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
}

fn truthy(source: &str) {
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
}

#[test]
fn iterable_path_constructs_with_no_arguments_defines_elements_and_sets_length() {
    truthy(
        "var args;function C(){args=arguments.length;this.made=true}\
         var a=Array.from.call(C,[7,8]);\
         a instanceof C&&args===0&&a.made&&a[0]===7&&a[1]===8&&a.length===2\
         &&Object.getOwnPropertyDescriptor(a,'0').enumerable",
    );
}

#[test]
fn array_like_path_constructs_with_the_length() {
    truthy(
        "var seen;function C(n){seen=[arguments.length,n]}\
         var a=Array.from.call(C,{length:3,0:'a',1:'b',2:'c'});\
         a instanceof C&&seen.join()==='1,3'&&a.length===3&&a[2]==='c'",
    );
    truthy(
        "function C(n){this.n=n}var a=Array.from.call(C,{length:2,0:1,1:2},x=>x*2);\
         a.n===2&&a[0]===2&&a[1]===4",
    );
}

#[test]
fn a_non_constructor_this_builds_a_plain_array() {
    truthy(
        "var a=Array.from.call({},[1,2]);Array.isArray(a)&&a.length===2&&a[1]===2\
         &&Object.getPrototypeOf(a)===Array.prototype",
    );
    truthy("var b=Array.from.call(()=>{},{length:1,0:'x'});Array.isArray(b)&&b[0]==='x'");
    truthy("var c=Array.from.call(undefined,[3]);Array.isArray(c)&&c[0]===3");
}

#[test]
fn subclasses_get_instances_of_the_subclass() {
    truthy(
        "class S extends Array{}var s=S.from([1,2,3]);\
         s instanceof S&&s.length===3&&s[2]===3",
    );
    truthy("Array.from([1,2]).constructor===Array");
}

#[test]
fn getting_the_iterator_method_precedes_constructing() {
    truthy(
        "var log=[];function C(){log.push('construct')}\
         var it={get [Symbol.iterator](){log.push('get');return function(){\
           log.push('iter');return{next(){return{done:true}}}}}};\
         Array.from.call(C,it);log.join()==='get,construct,iter'",
    );
}

#[test]
fn a_throwing_constructor_propagates_before_the_iterator_is_opened() {
    truthy(
        "var opened=false;function C(){throw new RangeError('c')}\
         var it={[Symbol.iterator](){opened=true;return{next(){return{done:true}}}}};\
         var caught;try{Array.from.call(C,it)}catch(e){caught=e}\
         caught instanceof RangeError&&!opened",
    );
}

#[test]
fn element_definition_failure_throws_and_closes_the_iterator() {
    truthy(
        "var closed=0;function C(){Object.freeze(this)}\
         var it={[Symbol.iterator](){return{next(){return{done:false,value:1}},\
           return(){closed++;return{}}}}};\
         var caught;try{Array.from.call(C,it)}catch(e){caught=e}\
         caught instanceof TypeError&&closed===1",
    );
    truthy(
        "function C(){Object.freeze(this)}var caught;\
         try{Array.from.call(C,{length:1,0:1})}catch(e){caught=e}caught instanceof TypeError",
    );
}

#[test]
fn a_failing_length_set_throws_without_closing_the_finished_iterator() {
    truthy(
        "var closed=0;function C(){Object.defineProperty(this,'length',{value:0,writable:false})}\
         var it={[Symbol.iterator](){var n=0;return{next(){return n++<1?{done:false,value:1}:{done:true}},\
           return(){closed++;return{}}}}};\
         var caught;try{Array.from.call(C,it)}catch(e){caught=e}\
         caught instanceof TypeError&&closed===0",
    );
    truthy(
        "function C(){Object.defineProperty(this,'length',{value:0,writable:false})}var caught;\
         try{Array.from.call(C,{length:0})}catch(e){caught=e}caught instanceof TypeError",
    );
}

#[test]
fn a_constructor_result_is_used_as_is_even_when_it_is_not_this() {
    truthy(
        "var target={};function C(){return target}var a=Array.from.call(C,[5]);\
         a===target&&target[0]===5&&target.length===1",
    );
}

#[test]
fn custom_constructor_results_survive_collection_on_every_allocation() {
    let source = "function C(){this.tag='c'}\
        var a=Array.from.call(C,Array.from({length:100},(_,i)=>i),function(x){return{x}});\
        a.tag==='c'&&a.length===100&&a[0].x===0&&a[99].x===99";
    assert_eq!(evaluate_gc_stress(source).unwrap(), Value::Bool(true));
}
