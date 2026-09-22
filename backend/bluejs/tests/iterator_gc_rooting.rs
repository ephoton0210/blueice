// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Iterator helpers (`Iterator.zip`, `Iterator.zipKeyed`, ...) must root every
//! value they hold across allocation: each script must give the same result
//! under the ordinary nursery and under a one-object nursery, where every
//! allocation may collect.
use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn evaluate(source: &str, nursery_capacity: Option<usize>) -> Result<Value, String> {
    evaluate_with_major_threshold(source, nursery_capacity, None)
}

fn evaluate_with_major_threshold(
    source: &str,
    nursery_capacity: Option<usize>,
    major_threshold_bytes: Option<usize>,
) -> Result<Value, String> {
    let mut config = VmConfig::default();
    if let Some(capacity) = nursery_capacity {
        config.heap.nursery_capacity = capacity;
    }
    if let Some(bytes) = major_threshold_bytes {
        config.heap.major_threshold_bytes = bytes;
    }
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .map_err(|error| format!("{error:?}"))
}

fn gc_stress_matches_ordinary(source: &str) {
    let ordinary = evaluate(source, None);
    assert_eq!(ordinary, Ok(Value::Bool(true)), "ordinary mode: {source}");
    assert_eq!(
        evaluate(source, Some(1)),
        ordinary,
        "GC-stress mode: {source}"
    );
}

/// A source whose `return` method allocates and throws, so closing it while
/// another completion is in flight gives the collector a chance to run.
const CLOSABLE: &str = "var log = [];\
    class ExpectedError extends Error {}\
    class FlightError extends Error {}\
    function closable(name) {\
      return {next() { return {done: true}; },\
              return() { log.push(name); throw new ExpectedError(); }};\
    }";

#[test]
fn zip_longest_keeps_the_padding_option_alive_while_the_source_iterator_is_read() {
    gc_stress_matches_ordinary(
        "var it = Iterator.zip([[1], [2, 3]], {mode: 'longest', get padding() { return [8, 9]; }});\
         var a = it.next().value, b = it.next().value, c = it.next();\
         a.join() === '1,2' && b.join() === '8,3' && c.done === true",
    );
}

#[test]
fn zip_keyed_longest_keeps_the_padding_option_alive() {
    gc_stress_matches_ordinary(
        "var it = Iterator.zipKeyed({a: [1, 2, 3], b: [4]},\
           {mode: 'longest', get padding() { return {a: 'x', b: 'pad'}; }});\
         var first = it.next().value, second = it.next().value, third = it.next().value;\
         first.a === 1 && first.b === 4 && second.a === 2 && second.b === 'pad'\
           && third.a === 3 && third.b === 'pad' && it.next().done",
    );
}

#[test]
fn zip_closes_opened_iterators_while_holding_the_outer_step_error() {
    gc_stress_matches_ordinary(&format!(
        "{CLOSABLE}\
         var items = [closable('first'), closable('second')], i = 0;\
         var iterables = {{[Symbol.iterator]() {{ return this; }},\
           next() {{ if (i < items.length) return {{done: false, value: items[i++]}};\
                    throw new FlightError(); }},\
           return() {{ log.push('outer'); return {{}}; }}}};\
         var caught;\
         try {{ Iterator.zip(iterables); }} catch (e) {{ caught = e; }}\
         log.join() === 'second,first' && caught instanceof FlightError"
    ));
}

#[test]
fn zip_closes_every_iterator_while_holding_the_flattenable_error() {
    gc_stress_matches_ordinary(&format!(
        "{CLOSABLE}\
         var caught;\
         try {{ Iterator.zip([closable('first'), closable('second'), 5]); }}\
         catch (e) {{ caught = e; }}\
         log.join() === 'second,first' && caught instanceof TypeError"
    ));
    gc_stress_matches_ordinary(&format!(
        "{CLOSABLE}\
         var bad = {{get [Symbol.iterator]() {{ throw new FlightError(); }}}};\
         var caught;\
         try {{ Iterator.zip([closable('first'), bad]); }} catch (e) {{ caught = e; }}\
         log.join() === 'first' && caught instanceof FlightError"
    ));
}

#[test]
fn zip_keyed_closes_every_iterator_while_holding_the_abrupt_completion() {
    gc_stress_matches_ordinary(&format!(
        "{CLOSABLE}\
         var source = {{first: closable('first'), second: closable('second')}};\
         Object.defineProperty(source, 'third',\
           {{enumerable: true, get() {{ throw new FlightError(); }}}});\
         var caught;\
         try {{ Iterator.zipKeyed(source); }} catch (e) {{ caught = e; }}\
         log.join() === 'second,first' && caught instanceof FlightError"
    ));
    gc_stress_matches_ordinary(&format!(
        "{CLOSABLE}\
         var iterables = new Proxy(\
           {{first: closable('first'), second: closable('second'), third: null}}, {{\
           getOwnPropertyDescriptor(target, key) {{\
             if (key === 'third') throw new FlightError();\
             return Reflect.getOwnPropertyDescriptor(target, key);\
           }}}});\
         var caught;\
         try {{ Iterator.zipKeyed(iterables); }} catch (e) {{ caught = e; }}\
         log.join() === 'second,first' && caught instanceof FlightError"
    ));
    gc_stress_matches_ordinary(&format!(
        "{CLOSABLE}\
         var caught;\
         try {{ Iterator.zipKeyed({{first: closable('first'), second: 7}}); }}\
         catch (e) {{ caught = e; }}\
         log.join() === 'first' && caught instanceof TypeError"
    ));
}

#[test]
fn zip_strict_results_and_length_mismatch_are_stable_under_gc_stress() {
    gc_stress_matches_ordinary(
        "var strict = Iterator.zip([[1, 2], [3, 4]], {mode: 'strict'});\
         var a = strict.next().value, b = strict.next().value, c = strict.next();\
         var threw = false;\
         var uneven = Iterator.zip([[1], [3, 4]], {mode: 'strict'});\
         uneven.next();\
         try { uneven.next(); } catch (e) { threw = e instanceof TypeError; }\
         a.join() === '1,3' && b.join() === '2,4' && c.done && threw",
    );
}

/// A major collection is triggered by managed-byte growth, not by the nursery
/// filling, so it can land on any property write that grows an object rather
/// than only on allocations. The first one fires when the run's bytes reach
/// the configured threshold, so sweeping the threshold across the bytes
/// `exercise` adds after `setup` moves that collection across every such
/// write. Both bounds are found by bisecting for the smallest threshold at
/// which no major collection beyond the unconditional ones runs, i.e. each
/// script's peak managed bytes.
fn survives_major_collections_at_every_property_write(setup: &str, exercise: &str) {
    let run = |source: &str, threshold: usize| {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = 1;
        config.heap.major_threshold_bytes = threshold;
        let mut vm = Vm::new(config).unwrap();
        let result = vm.execute(&compile(&parse(source).unwrap()).unwrap());
        (result, vm.heap().stats().major_collections)
    };
    let peak_bytes = |source: &str| {
        let ceiling = VmConfig::default().heap.max_heap_bytes;
        let (result, baseline) = run(source, ceiling);
        assert_eq!(result, Ok(Value::Bool(true)), "{source}");
        let (mut low, mut high) = (1, ceiling);
        while low < high {
            let middle = low + (high - low) / 2;
            if run(source, middle).1 == baseline {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        low
    };
    let source = format!("{setup}{exercise}");
    let (start, end) = (peak_bytes(&format!("{setup}true")), peak_bytes(&source));
    assert!(start <= end, "the exercise must add bytes");
    for threshold in (start.saturating_sub(64)..end + 64).step_by(4) {
        let (result, _) = run(&source, threshold);
        assert_eq!(
            result,
            Ok(Value::Bool(true)),
            "major threshold {threshold}: {source}"
        );
    }
}

#[test]
fn zip_result_arrays_stay_alive_when_the_first_step_grows_the_helper_state() {
    survives_major_collections_at_every_property_write(
        "var it = Iterator.zip([[1, 2], [3, 4, 5]], {mode: 'longest', padding: ['a', 'b']});",
        "var first = it.next().value, second = it.next().value, third = it.next().value;\
         first.join() === '1,3' && second.join() === '2,4' && third.join() === 'a,5'",
    );
}

#[test]
fn zip_keyed_result_objects_stay_alive_when_the_first_step_grows_the_helper_state() {
    survives_major_collections_at_every_property_write(
        "var it = Iterator.zipKeyed({a: [1, 2], b: [3, 4, 5]}, {mode: 'longest', padding: {a: 'x'}});",
        "var first = it.next().value, second = it.next().value, third = it.next().value;\
         first.a === 1 && first.b === 3 && second.b === 4 && third.a === 'x' && third.b === 5",
    );
}
