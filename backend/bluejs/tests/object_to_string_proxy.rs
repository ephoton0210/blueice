// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Object.prototype.toString` builtin tags and `Array.prototype.toString`'s
//! fallback to it, for Proxy receivers, through the real parse/compile/execute
//! path.
use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn run(source: &str, nursery_capacity: usize) -> Result<Value, RuntimeError> {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = nursery_capacity;
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
}

/// The ordinary run, then the same source with a one-object nursery.
fn truthy(source: &str) {
    for nursery in [VmConfig::default().heap.nursery_capacity, 1] {
        assert_eq!(run(source, nursery), Ok(Value::Bool(true)), "{source}");
    }
}

const THREW: &str = "function threw(f,E){try{f()}catch(e){return e instanceof E}return false}";

#[test]
fn object_to_string_inspects_internal_slots_of_the_proxy_itself() {
    truthy(
        "var tag=function(value){return Object.prototype.toString.call(value)};\
         tag(new Proxy(new Date,{}))==='[object Object]'\
         &&tag(new Proxy(/x/,{}))==='[object Object]'\
         &&tag(new Proxy(new Error,{}))==='[object Object]'\
         &&tag(new Proxy(new String(''),{}))==='[object Object]'\
         &&tag(new Proxy(new Number(1),{}))==='[object Object]'\
         &&tag(new Proxy((function(){return arguments})(),{}))==='[object Object]'\
         &&tag(new Proxy([],{}))==='[object Array]'\
         &&tag(new Proxy(new Proxy([],{}),{}))==='[object Array]'\
         &&tag(new Proxy(function(){},{}))==='[object Function]'\
         &&tag(new Proxy({},{}))==='[object Object]'",
    );
}

#[test]
fn object_to_string_on_a_revoked_proxy_throws() {
    truthy(&format!(
        "{THREW}var revocable=Proxy.revocable([],{{}});revocable.revoke();\
         threw(function(){{Object.prototype.toString.call(revocable.proxy)}},TypeError)"
    ));
}

#[test]
fn array_to_string_falls_back_to_the_intrinsic_object_to_string() {
    truthy(
        "delete Object.prototype.toString;\
         Array.prototype.toString.call({join:null})==='[object Object]'\
         &&Array.prototype.toString.call({join:Symbol()})==='[object Object]'\
         &&Array.prototype.toString.call(new Proxy(new Date,{}))==='[object Object]'\
         &&Array.prototype.toString.call(new Proxy(()=>{},{}))==='[object Function]'",
    );
}
