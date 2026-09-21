// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `yield` and `await` inside a template placeholder, and an operand-less
//! `yield` before a conditional's colon: the placeholder is parsed again in
//! the enclosing function's context. Every script also runs under a
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
    assert_eq!(
        evaluate(source, None),
        Ok(Value::Bool(true)),
        "ordinary mode: {source}"
    );
    assert_eq!(
        evaluate(source, Some(1)),
        Ok(Value::Bool(true)),
        "GC-stress mode: {source}"
    );
}

#[test]
fn yield_inside_a_template_placeholder_suspends_the_enclosing_generator() {
    truthy(
        "var str;\
         function* g() { str = `1${ yield }3${ 4 }5`; return `v${yield 'x'}`; }\
         var it = g();\
         var first = it.next(), second = it.next(2), third = it.next('!');\
         first.done === false && first.value === undefined\
           && second.value === 'x' && str === '12345'\
           && third.value === 'v!' && third.done === true",
    );
}

#[test]
fn yield_without_an_operand_may_precede_a_conditional_colon() {
    truthy(
        "function* g() { return (yield) ? yield : yield; }\
         var it = g();\
         it.next();\
         var consequent = it.next(true);\
         var result = it.next('b');\
         consequent.done === false && result.done === true && result.value === 'b'",
    );
}

#[test]
fn a_template_placeholder_inherits_the_generator_and_async_context_of_its_function() {
    assert!(parse("async function f() { return `a${await 1}b`; }").is_ok());
    assert!(parse("function* g() { return tag`a${yield}b`; }").is_ok());
    truthy(
        "function* g() { return tag`a${yield 1}b`; function tag(strings, value) { return value; } }\
         var it = g();\
         var first = it.next(), second = it.next('v');\
         first.value === 1 && second.value === 'v' && second.done === true",
    );
}
