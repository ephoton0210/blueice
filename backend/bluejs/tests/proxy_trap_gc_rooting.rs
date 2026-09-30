// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A Proxy trap read through a handler whose own `get` returns a fresh
//! function is held nowhere else: it must stay rooted while the arguments for
//! the trap call are allocated. Each script must give the same result under
//! every collection schedule.
use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn evaluate(source: &str, nursery_capacity: usize, major_threshold_bytes: usize) -> Value {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = nursery_capacity;
    config.heap.major_threshold_bytes = major_threshold_bytes;
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| {
            panic!("nursery {nursery_capacity}, major threshold {major_threshold_bytes}: {error:?}")
        })
}

const FRESH_TRAPS: &str = "\
    function forwarding(target) {\
      return new Proxy(target, new Proxy({}, {\
        get(t, trap) { return (t2, ...rest) => Reflect[trap](t2, ...rest); }\
      }));\
    }";

fn stress(body: &str) {
    let source = format!("{FRESH_TRAPS} {body}");
    for nursery in [1, 2, 3] {
        for threshold in [20_000, 60_000, 120_000, 256 * 1024] {
            assert_eq!(
                evaluate(&source, nursery, threshold),
                Value::Bool(true),
                "nursery {nursery}, major threshold {threshold}"
            );
        }
    }
}

#[test]
fn define_property_trap_survives_descriptor_allocation() {
    stress(
        "var proxy = forwarding({}), n = 0;\
         for (var i = 0; i < 40; i++) {\
           Object.defineProperty(proxy, 'k' + i, {value: i, configurable: true}); n += proxy['k' + i];\
         }\
         n === 780",
    );
}

#[test]
fn apply_and_construct_traps_survive_argument_array_allocation() {
    stress(
        "var callable = forwarding(function (a, b) { return a + b; }), n = 0;\
         for (var i = 0; i < 40; i++) { n += callable(i, 1); }\
         var made = new (forwarding(function C(x) { this.x = x; }))(7);\
         n === 820 && made.x === 7",
    );
}
