// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A module-goal program run with `Vm::execute` (rather than through the
//! module-graph loader) has no continuation to suspend at a top-level
//! `await`, so the await drains the promise jobs itself and reads the
//! settled result.
use blueice_bluejs::{compile_module, parse_module, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile_module(&parse_module(source).unwrap()).unwrap())
}

#[test]
fn awaiting_reads_a_settled_promise_synchronously() {
    assert_eq!(
        evaluate("var v = await Promise.resolve(5); v + 1"),
        Ok(Value::Number(6.0))
    );
    assert_eq!(evaluate("var v = await 5; v"), Ok(Value::Number(5.0)));
    assert_eq!(
        evaluate("var v = await { then(resolve) { resolve('thenable'); } }; v.length"),
        Ok(Value::Number(8.0))
    );
}

#[test]
fn awaiting_a_rejection_throws_the_reason() {
    assert_eq!(
        evaluate("await Promise.reject(7)"),
        Err(RuntimeError::Thrown(Value::Number(7.0)))
    );
}

#[test]
fn awaiting_a_promise_that_never_settles_is_unsupported() {
    assert_eq!(
        evaluate("await new Promise(() => {})"),
        Err(RuntimeError::Unsupported("pending await continuation"))
    );
}
