// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `reverse`, `sort`, `splice` and `unshift` on primitive and array-like
//! receivers: the returned object, the pairs `reverse` leaves alone, and
//! lengths clamped to 2^53-1, through the real parse/compile/execute path.
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
fn reverse_and_sort_return_the_object_coercion_of_a_primitive_receiver() {
    truthy(
        "var ok=true;\
         [true,false,0,'',Symbol(),0n].forEach(function(primitive){\
         ok=ok&&Array.prototype.reverse.call(primitive) instanceof Object\
         &&Array.prototype.sort.call(primitive) instanceof Object\
         &&typeof Array.prototype.reverse.call(primitive)==='object'});\
         var plain={length:0};\
         ok&&Array.prototype.reverse.call(true) instanceof Boolean\
         &&Array.prototype.sort.call(0) instanceof Number\
         &&Array.prototype.sort.call('') instanceof String\
         &&Array.prototype.reverse.call(plain)===plain&&Array.prototype.sort.call(plain)===plain",
    );
}

#[test]
fn reverse_leaves_a_pair_of_absent_indices_untouched() {
    truthy(
        "var log=[];var target={0:'a',3:'d',length:6};\
         var proxy=new Proxy(target,{deleteProperty:function(t,k){log.push('delete '+k);return delete t[k]},\
         defineProperty:function(t,k,d){log.push('define '+k);return Reflect.defineProperty(t,k,d)}});\
         Array.prototype.reverse.call(proxy);\
         log.join()==='delete 0,define 5,define 2,delete 3'&&target[5]==='a'&&target[2]==='d'\
         &&!(0 in target)&&!(1 in target)&&!(3 in target)&&!(4 in target)",
    );
}

#[test]
fn splice_and_unshift_without_items_clamp_the_length_to_two_to_the_fifty_third_minus_one() {
    truthy(
        "var limit=2**53-1;var ok=true;\
         [limit,2**53,2**53+2,Infinity].forEach(function(length){\
         var spliced={length:length};var shifted={length:length};\
         var removed=Array.prototype.splice.call(spliced);\
         Array.prototype.unshift.call(shifted);\
         ok=ok&&spliced.length===limit&&shifted.length===limit\
         &&Array.isArray(removed)&&removed.length===0});\
         var array=[1,2,3];var none=array.splice();\
         ok&&none.length===0&&array.join()==='1,2,3'&&[1,2,3].splice(undefined).join()==='1,2,3'",
    );
}

#[test]
fn unshift_with_items_still_rejects_a_length_past_the_integer_limit() {
    truthy(&format!(
        "{THREW}threw(function(){{Array.prototype.unshift.call({{length:2**53-1}},1)}},TypeError)"
    ));
}
