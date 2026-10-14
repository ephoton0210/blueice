// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn message(body: AsyncBody) -> String {
    match body {
        AsyncBody::Failed(RuntimeError::TypeError(message)) => message,
        AsyncBody::Failed(other) => format!("{other:?}"),
        AsyncBody::Returned(_) => "returned".into(),
        AsyncBody::Awaiting { pc, handlers, .. } => {
            format!("awaiting at {pc} with {} handlers", handlers.len())
        }
    }
}

#[cfg_attr(test, test)]
fn an_async_function_body_can_only_return_await_or_fail() {
    let mut vm = Vm::default();
    let promise = vm.new_promise().unwrap();
    assert_eq!(
        message(AsyncBody::classify(Ok(InterpreterExit::Return(
            Value::Undefined
        )))),
        "returned"
    );
    assert_eq!(
        message(AsyncBody::classify(Ok(InterpreterExit::Await {
            promise,
            pc: 7,
            handlers: Vec::new(),
        }))),
        "awaiting at 7 with 0 handlers"
    );
    assert_eq!(
        message(AsyncBody::classify(Err(RuntimeError::RangeError(
            "r".into()
        )))),
        "RangeError(\"r\")"
    );
    assert_eq!(
        message(AsyncBody::classify(Ok(InterpreterExit::Yield {
            value: Value::Undefined,
            pc: 0,
            iterators: Vec::new(),
            handlers: Vec::new(),
        }))),
        "yield requires an async generator function"
    );
    assert_eq!(
        message(AsyncBody::classify(Ok(InterpreterExit::Suspend {
            pc: 0,
            iterators: Vec::new(),
            handlers: Vec::new(),
        }))),
        "an ordinary async function has no entry suspend"
    );
}

#[cfg_attr(test, test)]
fn an_async_function_fails_its_first_await_when_continuation_ids_run_out() {
    let mut vm = Vm {
        next_async_continuation: u64::MAX,
        ..Vm::default()
    };
    let code =
        crate::compile(&crate::parse("(async function () { await 1; })()").unwrap()).unwrap();
    assert_eq!(vm.execute(&code), Err(RuntimeError::InstructionLimit));
}

impl crate::Vm {
    /// Runs retained unit contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_closure_execution_contracts() {
        an_async_function_body_can_only_return_await_or_fail();
        an_async_function_fails_its_first_await_when_continuation_ids_run_out();
    }
}
