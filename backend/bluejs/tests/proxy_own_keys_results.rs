// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! OwnPropertyKeys validates trap results before inspecting an array-like list.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn execute(source: &str, nursery: usize) -> Value {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = nursery;
    Vm::new(config)
        .unwrap()
        .execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn primitive_own_keys_results_are_rejected_through_every_public_consumer() {
    let mut failures = Vec::new();
    for nursery in [VmConfig::default().heap.nursery_capacity, 1] {
        for primitive in [
            "undefined",
            "null",
            "false",
            "true",
            "0",
            "1",
            "NaN",
            "0n",
            "Symbol()",
            "''",
            "'ab'",
        ] {
            for consumer in [
                "Reflect.ownKeys(subject)",
                "Object.keys(subject)",
                "Object.getOwnPropertyNames(subject)",
                "Object.getOwnPropertySymbols(subject)",
                "Object.assign({},subject)",
                "({...subject})",
                "(()=>{var {...rest}=subject;return rest;})()",
                "(()=>{for(var key in subject){}return 0;})()",
            ] {
                let source=format!("var subject=new Proxy({{}},{{ownKeys(){{return {primitive};}}}});var threw=false;try{{{consumer};}}catch(error){{threw=error instanceof TypeError;}}threw;");
                if execute(&source, nursery) != Value::Bool(true) {
                    failures.push((nursery, primitive, consumer));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn primitive_trap_results_never_run_boxed_length_getters() {
    for nursery in [VmConfig::default().heap.nursery_capacity, 1] {
        for (prototype, primitive) in [
            ("Number.prototype", "1"),
            ("Boolean.prototype", "true"),
            ("BigInt.prototype", "0n"),
            ("Symbol.prototype", "Symbol()"),
        ] {
            let source=format!("var reads=0;Object.defineProperty({prototype},'length',{{get(){{reads++;return 0;}}}});var subject=new Proxy({{}},{{ownKeys(){{return {primitive};}}}});var threw=false;try{{Reflect.ownKeys(subject);}}catch(error){{threw=error instanceof TypeError;}}threw && reads===0;");
            assert_eq!(execute(&source, nursery), Value::Bool(true), "{prototype}");
        }
    }
}

#[test]
fn object_valued_own_keys_results_keep_array_like_order_and_original_errors() {
    for nursery in [VmConfig::default().heap.nursery_capacity, 1] {
        assert_eq!(execute("var subject=new Proxy({},{ownKeys(){return {0:'second',1:'first',length:2};}});Reflect.ownKeys(subject).join(',')==='second,first';",nursery),Value::Bool(true));
        assert_eq!(execute("var original={};var subject=new Proxy({},{ownKeys(){return {get length(){throw original;}};}});var caught=false;try{Reflect.ownKeys(subject);}catch(error){caught=error===original;}caught;",nursery),Value::Bool(true));
    }
}
