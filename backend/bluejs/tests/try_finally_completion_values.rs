// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The completion value of `try`/`catch`/`finally` (§14.15.3): a `finally`
//! that completes normally does not change it, whichever way the try or catch
//! block completed (normally, or abruptly with `break`/`continue`); only an
//! abrupt `finally` supplies its own value.
use blueice_bluejs::{compile, parse, Value, Vm};

fn value_of(source: &str) -> Value {
    let program = parse(&format!("eval({source:?})")).unwrap();
    Vm::default()
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn a_normal_finally_keeps_the_value_of_a_try_that_broke_or_continued() {
    assert_eq!(
        value_of("while (true) { try { 'try'; break; } finally { 'finally'; } }"),
        text("try")
    );
    assert_eq!(
        value_of("do { try { 'try'; continue; } finally { 'finally'; } } while (false);"),
        text("try")
    );
    // With no value of its own the break completes with `undefined`.
    assert_eq!(
        value_of("while (true) { try { break; } finally { 'finally'; } }"),
        Value::Undefined
    );
    assert_eq!(
        value_of("do { try { continue; } finally { 'finally'; } } while (false);"),
        Value::Undefined
    );
}

#[test]
fn a_normal_finally_keeps_the_value_of_a_catch_that_broke_or_continued() {
    assert_eq!(
        value_of(
            "while (true) { try { 'try'; throw 'e'; } catch (e) { 'catch'; break; } finally { 'finally'; } }"
        ),
        text("catch")
    );
    assert_eq!(
        value_of(
            "do { try { 'try'; throw 'e'; } catch (e) { 'catch'; continue; } finally { 'finally'; } } while (false);"
        ),
        text("catch")
    );
    assert_eq!(
        value_of(
            "while (true) { try { 'try'; throw 'e'; } catch (e) { break; } finally { 'finally'; } }"
        ),
        Value::Undefined
    );
}

#[test]
fn an_abrupt_finally_supplies_its_own_value() {
    assert_eq!(
        value_of("while (true) { try { 'try'; break; } finally { 'finally'; break; } }"),
        text("finally")
    );
    assert_eq!(
        value_of("while (true) { try { 'try'; } finally { break; } }"),
        Value::Undefined
    );
    assert_eq!(
        value_of("do { try { 'try'; continue; } finally { 'finally'; continue; } } while (false);"),
        text("finally")
    );
}

#[test]
fn a_finally_that_runs_a_loop_or_a_nested_try_keeps_the_pending_value() {
    assert_eq!(
        value_of(
            "while (true) { try { 'try'; break; } finally { 'a'; for (var i = 0; i < 2; i++) { 'b'; } } }"
        ),
        text("try")
    );
    assert_eq!(
        value_of(
            "while (true) { try { 'outer'; break; } finally { try { 'inner'; } finally { 'innermost'; } } }"
        ),
        text("outer")
    );
    // A break out of a nested finally must not disturb the outer restore.
    assert_eq!(
        value_of(
            "while (true) { try { 'outer'; break; } finally { do { try { 'x'; } finally { break; } } while (0); } }"
        ),
        text("outer")
    );
}

#[test]
fn a_generator_can_yield_inside_a_finally_that_runs_for_a_break() {
    assert_eq!(
        value_of(
            "function* g() { while (true) { try { 'try'; break; } finally { yield 1; } } }
             var it = g(); var a = it.next(); var b = it.next(); a.value === 1 && !a.done && b.done"
        ),
        Value::Bool(true)
    );
}
