// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-iteration environments of a `for (let ...; ...; ...)` loop
//! (§14.7.4.3 ForBodyEvaluation / CreatePerIterationEnvironment): the bindings
//! are copied once after the initializer runs and again after every
//! iteration's body, before the increment.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn truthy(source: &str) {
    for capacity in [None, Some(1)] {
        let mut config = VmConfig::default();
        if let Some(capacity) = capacity {
            config.heap.nursery_capacity = capacity;
        }
        let result = Vm::new(config)
            .unwrap()
            .execute(&compile(&parse(source).unwrap()).unwrap());
        assert_eq!(result, Ok(Value::Bool(true)), "{capacity:?}: {source}");
    }
}

#[test]
fn a_closure_made_by_the_initializer_keeps_the_original_bindings() {
    truthy(
        "var probeBefore, probeTest, probeIncr, probeBody; var run = true;
         for (let x = 'outside', _ = probeBefore = function() { return x; };
              run && (x = 'inside', probeTest = function() { return x; });
              probeIncr = function() { return x; })
           probeBody = function() { return x; }, run = false;
         probeBefore() === 'outside' && probeTest() === 'inside'
           && probeBody() === 'inside' && probeIncr() === 'inside'",
    );
}

#[test]
fn each_iteration_gets_its_own_copy() {
    truthy(
        "var fs = []; for (let i = 0; i < 3; i++) fs.push(() => i);
         fs.map(f => f()).join() === '0,1,2'",
    );
    truthy(
        "var fs = []; for (let i = 0; i < 3; fs.push(() => i), i++);
         fs.map(f => f()).join() === '1,2,3'",
    );
}

#[test]
fn a_const_or_var_head_needs_no_copies_and_still_works() {
    truthy("var n = 0; for (const c = 5; n < 2; n++) ; n === 2");
    truthy("var fs = []; for (var i = 0; i < 2; i++) fs.push(() => i); fs.map(f => f()).join() === '2,2'");
}
