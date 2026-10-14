// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Suspending a generator stores its frame in the heap, which can trigger a
//! major collection: the value being yielded and the caller's frame must stay
//! rooted across it. Each script must give the same result under every
//! collection schedule.
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

// The loop runs inside a function so that a failure unwinds through an
// ordinary call frame, which must find its saved dynamic environment intact.
const PERMUTATIONS: &str = "\
    function* Permutations(items) {\
      if (items.length === 0) { yield []; }\
      else {\
        for (let i = 0; i < items.length; i++) {\
          let tail = items.slice(0);\
          let head = tail.splice(i, 1);\
          for (let e of Permutations(tail)) { yield head.concat(e); }\
        }\
      }\
    }\
    function sortAll(words) {\
      let count = 0, sorted = 0;\
      for (let permutation of Permutations(words)) {\
        permutation.sort();\
        count++;\
        if (permutation.join() === '2112,bob,is,my,name') sorted++;\
      }\
      return count === 120 && sorted === 120;\
    }\
    sortAll([2112, 'bob', 'is', 'my', 'name'])";

#[test]
fn nested_generators_survive_every_collection_schedule() {
    for nursery in [1, 5] {
        for threshold in (20_000..300_000).step_by(40_000) {
            assert_eq!(
                evaluate(PERMUTATIONS, nursery, threshold),
                Value::Bool(true),
                "nursery {nursery}, major threshold {threshold}"
            );
        }
    }
}
