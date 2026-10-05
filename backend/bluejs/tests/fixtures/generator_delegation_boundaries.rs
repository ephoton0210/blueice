// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A stale delegation completion cannot consume a nondelegating frame.

use super::*;
use crate::{compile, parse};

fn interpreter_exit(kind: u8, promise: ObjectId) -> InterpreterExit {
    match kind {
        0 => InterpreterExit::Return(Value::Number(42.0)),
        1 => InterpreterExit::Yield {
            value: Value::Number(42.0),
            pc: 7,
            iterators: Vec::new(),
            handlers: Vec::new(),
        },
        2 => InterpreterExit::Suspend {
            pc: 7,
            iterators: Vec::new(),
            handlers: Vec::new(),
        },
        _ => InterpreterExit::Await {
            promise,
            pc: 7,
            handlers: Vec::new(),
        },
    }
}

#[cfg_attr(test, test)]
fn async_request_turns_separate_awaited_operands_and_reject_resumption_after_completion() {
    let value = Value::Number(42.0);
    assert_eq!(
        AsyncGeneratorRequestTurn::classify(AsyncGeneratorCompletion::Return(value.clone())),
        AsyncGeneratorRequestTurn::AwaitReturn(value.clone())
    );
    for (completion, expected) in [
        (
            AsyncGeneratorCompletion::Next(value.clone()),
            AsyncGeneratorRunCompletion::Next(value.clone()),
        ),
        (
            AsyncGeneratorCompletion::ReturnAwaited(value.clone()),
            AsyncGeneratorRunCompletion::ReturnAwaited(value.clone()),
        ),
        (
            AsyncGeneratorCompletion::ResumeReturn(value.clone()),
            AsyncGeneratorRunCompletion::ResumeReturn(value.clone()),
        ),
        (
            AsyncGeneratorCompletion::ResumeThrow(value.clone()),
            AsyncGeneratorRunCompletion::ResumeThrow(value.clone()),
        ),
        (
            AsyncGeneratorCompletion::Throw(value.clone()),
            AsyncGeneratorRunCompletion::Throw(value.clone()),
        ),
    ] {
        assert_eq!(
            AsyncGeneratorRequestTurn::classify(completion),
            AsyncGeneratorRequestTurn::Run(expected)
        );
    }
    for (completion, expected) in [
        (
            AsyncGeneratorCompletion::Next(value.clone()),
            CompletedAsyncGeneratorRequest::Next,
        ),
        (
            AsyncGeneratorCompletion::Return(value.clone()),
            CompletedAsyncGeneratorRequest::Return(value.clone()),
        ),
        (
            AsyncGeneratorCompletion::Throw(value.clone()),
            CompletedAsyncGeneratorRequest::Throw(value.clone()),
        ),
    ] {
        assert_eq!(
            CompletedAsyncGeneratorRequest::classify(completion),
            Ok(expected)
        );
    }
    for completion in [
        AsyncGeneratorCompletion::ReturnAwaited(value.clone()),
        AsyncGeneratorCompletion::ResumeReturn(value.clone()),
        AsyncGeneratorCompletion::ResumeThrow(value),
    ] {
        let mut vm = Vm::default();
        let generator = vm
            .execute_script(&compile(&parse("(async function*() {})()").unwrap()).unwrap())
            .unwrap()
            .object_id()
            .unwrap();
        let root = vm.heap.root(generator).unwrap();
        let target = vm.new_promise().unwrap();
        let target_root = vm.heap.root(target).unwrap();
        vm.heap
            .set_generator_state(generator, GeneratorState::Done)
            .unwrap();
        let mut control = vm.heap.async_generator_control(generator).unwrap().unwrap();
        control.status = AsyncGeneratorStatus::Completed;
        control.next_request_id = 2;
        control.requests.push_back(AsyncGeneratorRequest {
            id: 1,
            target,
            completion,
        });
        vm.heap
            .set_async_generator_control(generator, control)
            .unwrap();
        assert_eq!(
            vm.resume_async_generator_next(generator),
            Err(RuntimeError::Unsupported(
                "a completed async generator cannot retain a resumed request"
            ))
        );
        let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
        assert_eq!(control.requests.len(), 1);
        assert_eq!(control.requests.front().unwrap().target, target);
        assert!(matches!(
            vm.promises[&target].status,
            PromiseStatus::Pending
        ));
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        vm.heap.unroot(target_root).unwrap();
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn abrupt_generator_completion_preserves_resume_targets_values_and_errors() {
    for (action, expected) in [
        (
            CompletionAction::Continue,
            GeneratorAbruptAction::Resume(None),
        ),
        (
            CompletionAction::Jump(17),
            GeneratorAbruptAction::Resume(Some(17)),
        ),
        (
            CompletionAction::Return(Value::Number(42.0)),
            GeneratorAbruptAction::Return(Value::Number(42.0)),
        ),
    ] {
        assert_eq!(GeneratorAbruptAction::classify(Ok(action)), Ok(expected));
    }
    for action in [
        CompletionAction::TailRecur(Vec::new()),
        CompletionAction::TailCall(Vec::new()),
    ] {
        assert_eq!(
            GeneratorAbruptAction::classify(Ok(action)),
            Err(RuntimeError::TypeError(
                "generator cannot tail recur across an abrupt resume".into()
            ))
        );
    }
    let thrown = RuntimeError::Thrown(Value::Number(7.0));
    assert_eq!(
        GeneratorAbruptAction::classify(Ok(CompletionAction::Throw(thrown.clone()))),
        Err(thrown.clone())
    );
    assert_eq!(
        GeneratorAbruptAction::classify(Err(thrown.clone())),
        Err(thrown)
    );
}

#[cfg_attr(test, test)]
fn interpreter_exit_protocol_distinguishes_generator_entry_and_resume() {
    let mut vm = Vm::default();
    let promise = vm.new_promise().unwrap();
    for kind in 0..4 {
        let entry =
            std::panic::catch_unwind(|| generator_entry_exit(interpreter_exit(kind, promise)));
        if kind == 2 {
            let (pc, iterators, handlers) = entry.unwrap();
            assert_eq!(pc, 7);
            assert!(iterators.is_empty() && handlers.is_empty());
        } else {
            assert!(entry.is_err(), "generator entry accepted exit {kind}");
        }
        let resumed = std::panic::catch_unwind(|| {
            GeneratorRunExit::from_interpreter(interpreter_exit(kind, promise))
        });
        if kind == 2 {
            assert!(
                resumed.is_err(),
                "a resumed generator accepted an entry suspension"
            );
        } else {
            match resumed.unwrap() {
                GeneratorRunExit::Return(value) => assert_eq!(value, Value::Number(42.0)),
                GeneratorRunExit::Yield {
                    value,
                    pc,
                    iterators,
                    handlers,
                } => {
                    assert_eq!(value, Value::Number(42.0));
                    assert_eq!(pc, 7);
                    assert!(iterators.is_empty() && handlers.is_empty());
                }
                GeneratorRunExit::Await {
                    promise: target,
                    pc,
                    handlers,
                } => {
                    assert_eq!(target, promise);
                    assert_eq!(pc, 7);
                    assert!(handlers.is_empty());
                }
            }
        }
    }
}

fn state_kind(vm: &mut Vm, generator: ObjectId) -> u8 {
    let state = vm.heap.take_generator_state(generator).unwrap();
    let kind = match &state {
        GeneratorState::Start { .. } => 0,
        GeneratorState::Suspended { .. } => 1,
        GeneratorState::Running => 2,
        GeneratorState::Done => 3,
    };
    vm.heap.set_generator_state(generator, state).unwrap();
    kind
}

#[cfg_attr(test, test)]
fn generator_internal_boundaries_reject_invalid_receivers_without_consuming_state() {
    let mut vm = Vm::default();
    for receiver in [
        Value::Undefined,
        Value::Null,
        Value::Bool(false),
        Value::Number(0.0),
        Value::String("".into()),
        Value::BigInt(0.into()),
        Value::Symbol(JsSymbol::well_known("iterator")),
    ] {
        assert_eq!(
            vm.generator_next(&receiver, None, None),
            Err(RuntimeError::TypeError(
                "Generator next requires a generator".into()
            ))
        );
        assert_eq!(
            vm.generator_delegate_return(&receiver, Value::Undefined),
            Err(RuntimeError::TypeError(
                "Generator return requires a generator".into()
            ))
        );
        assert_eq!(
            vm.generator_return(&receiver, Value::Undefined),
            Err(RuntimeError::TypeError(
                "Generator return requires a generator".into()
            ))
        );
        assert_eq!(
            vm.generator_throw(&receiver, Value::Undefined),
            Err(RuntimeError::TypeError(
                "Generator throw requires a generator".into()
            ))
        );
    }
    let mut foreign = Vm::default();
    let foreign_object = foreign.heap.alloc_object(None).unwrap();
    let foreign_root = foreign.heap.root(foreign_object).unwrap();
    let local = vm.heap.alloc_object(None).unwrap();
    let local_root = vm.heap.root(local).unwrap();
    for object in [foreign_object, local] {
        let receiver = Value::Object(object);
        let error = RuntimeError::Heap(HeapError::InvalidObject(object));
        assert_eq!(vm.generator_next(&receiver, None, None), Err(error.clone()));
        assert_eq!(
            vm.generator_delegate_return(&receiver, Value::Undefined),
            Err(error.clone())
        );
        assert_eq!(
            vm.generator_return(&receiver, Value::Undefined),
            Err(error.clone())
        );
        assert_eq!(
            vm.generator_throw(&receiver, Value::Undefined),
            Err(error.clone())
        );
        assert_eq!(vm.close_async_generator(object), Err(error));
    }
    let generator = vm
        .execute_script(&compile(&parse("(async function* () {yield 1;})()").unwrap()).unwrap())
        .unwrap();
    let object = generator.object_id().unwrap();
    let root = vm.heap.root(object).unwrap();
    assert_eq!(
        vm.generator_next(&generator, None, None),
        Err(RuntimeError::TypeError(
            "await requires an async generator function".into()
        ))
    );
    assert_eq!(state_kind(&mut vm, object), 3);
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    vm.heap.unroot(root).unwrap();
    vm.heap.unroot(local_root).unwrap();
    foreign.heap.unroot(foreign_root).unwrap();
}

#[cfg_attr(test, test)]
fn async_scheduler_helpers_reject_foreign_heap_handles_before_queue_changes() {
    let mut owner = Vm::default();
    let foreign = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(foreign).unwrap();
    let mut vm = Vm::default();
    let promise = vm.new_promise().unwrap();
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(foreign)));
    for result in [
        vm.set_async_generator_status(foreign, AsyncGeneratorStatus::Completed),
        vm.complete_async_generator_request(
            foreign,
            promise,
            PromiseStatus::Fulfilled(Value::Undefined),
        ),
        vm.resume_async_generator_next(foreign),
        vm.async_generator_is_suspended_start(foreign).map(|_| ()),
        vm.replace_async_generator_request(
            foreign,
            AsyncGeneratorCompletion::Next(Value::Undefined),
        ),
        vm.finish_async_delegation(foreign, Value::Undefined),
    ] {
        assert_eq!(result, expected);
    }
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert!(vm.stack.is_empty());
    assert_eq!(
        vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
        Ok(Value::Number(42.0))
    );
    owner.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn request_replacement_checks_the_brand_and_requires_a_queue_head() {
    let mut vm = Vm::default();
    for source in ["(function* () {})()", "(async function* () {})()"] {
        let generator = vm
            .execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap()
            .object_id()
            .unwrap();
        let root = vm.heap.root(generator).unwrap();
        let before = state_kind(&mut vm, generator);
        let error = if source.starts_with("(async") {
            RuntimeError::Unsupported("missing async generator request")
        } else {
            RuntimeError::TypeError("Async generator receiver required".into())
        };
        assert_eq!(
            vm.replace_async_generator_request(
                generator,
                crate::heap::AsyncGeneratorCompletion::ResumeThrow(Value::Number(7.0)),
            ),
            Err(error)
        );
        assert_eq!(state_kind(&mut vm, generator), before);
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn closing_a_suspended_async_frame_closes_its_real_iterator_and_keeps_the_vm_reusable() {
    for failing_close in [false, true] {
        let mut vm = Vm::default();
        let close = if failing_close {
            "closed++; throw 'close';"
        } else {
            "closed++; return {done:true};"
        };
        let source = format!(
            "globalThis.closed = 0; var source = {{[Symbol.iterator]() {{return {{next() {{return {{value: 1, done: false}};}}, return() {{{close}}}}};}}}}; (async function* () {{for (var value of source) {{yield value;}}}})()"
        );
        let generator = vm
            .execute_script(&compile(&parse(&source).unwrap()).unwrap())
            .unwrap()
            .object_id()
            .unwrap();
        let root = vm.heap.root(generator).unwrap();
        vm.async_generator_request(
            &Value::Object(generator),
            Value::Undefined,
            NativeFunction::AsyncGeneratorNext,
        )
        .unwrap();
        vm.run_promise_jobs().unwrap();
        assert_eq!(state_kind(&mut vm, generator), 1);
        let expected = if failing_close {
            Err(RuntimeError::Thrown(Value::String("close".into())))
        } else {
            Ok(())
        };
        assert_eq!(vm.close_async_generator(generator), expected);
        assert_eq!(state_kind(&mut vm, generator), 3);
        assert_eq!(
            vm.lookup_global_name("closed").unwrap(),
            Some(Value::Number(1.0))
        );
        vm.close_async_generator(generator).unwrap();
        assert_eq!(
            vm.lookup_global_name("closed").unwrap(),
            Some(Value::Number(1.0))
        );
        assert_eq!(
            vm.execute_script(&compile(&parse("21 + 21").unwrap()).unwrap()),
            Ok(Value::Number(42.0))
        );
        vm.heap.unroot(root).unwrap();
    }
}

#[cfg_attr(test, test)]
fn stale_delegate_answers_preserve_start_suspended_and_completed_frames() {
    for asynchronous in [false, true] {
        for phase in 0..3 {
            let mut vm = Vm::default();
            let source = if asynchronous {
                "(async function* () {yield 1; return 42;})()"
            } else {
                "(function* () {yield 1; return 42;})()"
            };
            let generator = vm
                .execute_script(&compile(&parse(source).unwrap()).unwrap())
                .unwrap()
                .object_id()
                .unwrap();
            let root = vm.heap.root(generator).unwrap();
            for _ in 0..phase {
                if asynchronous {
                    vm.async_generator_request(
                        &Value::Object(generator),
                        Value::Undefined,
                        NativeFunction::AsyncGeneratorNext,
                    )
                    .unwrap();
                    vm.run_promise_jobs().unwrap();
                } else {
                    vm.generator_next(&Value::Object(generator), None, None)
                        .unwrap();
                }
            }
            let before = state_kind(&mut vm, generator);
            let outcome = if asynchronous {
                vm.finish_async_delegation(generator, Value::Number(7.0))
            } else {
                vm.finish_sync_delegation(generator, Value::Number(7.0))
            };
            assert_eq!(
                outcome,
                Err(RuntimeError::Unsupported(if asynchronous {
                    "lost async yield* delegation state"
                } else {
                    "lost synchronous yield* delegation state"
                }))
            );
            assert_eq!(state_kind(&mut vm, generator), before);
            if asynchronous {
                let promise = vm
                    .async_generator_request(
                        &Value::Object(generator),
                        Value::Undefined,
                        NativeFunction::AsyncGeneratorNext,
                    )
                    .unwrap()
                    .object_id()
                    .unwrap();
                vm.run_promise_jobs().unwrap();
                assert!(matches!(
                    vm.promises[&promise].status,
                    PromiseStatus::Fulfilled(_)
                ));
            } else {
                let result = vm
                    .generator_next(&Value::Object(generator), None, None)
                    .unwrap();
                assert_eq!(
                    vm.get_property(&result, &"value".into()).unwrap(),
                    match phase {
                        0 => Value::Number(1.0),
                        1 => Value::Number(42.0),
                        _ => Value::Undefined,
                    }
                );
            }
            vm.heap.unroot(root).unwrap();
        }
    }
}

impl Vm {
    /// Runs delegation lifecycle contracts only in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_generator_delegation_boundary_contracts() {
        staged_sync_delegation_refusals_retain_completed_values();
        staged_async_generator_refusals_preserve_awaited_values_and_delegation();
        async_scheduler_helpers_reject_foreign_heap_handles_before_queue_changes();
        async_request_turns_separate_awaited_operands_and_reject_resumption_after_completion();
        abrupt_generator_completion_preserves_resume_targets_values_and_errors();
        interpreter_exit_protocol_distinguishes_generator_entry_and_resume();
        generator_internal_boundaries_reject_invalid_receivers_without_consuming_state();
        request_replacement_checks_the_brand_and_requires_a_queue_head();
        closing_a_suspended_async_frame_closes_its_real_iterator_and_keeps_the_vm_reusable();
        stale_delegate_answers_preserve_start_suspended_and_completed_frames();
        async_generator_allocation_failures_preserve_queued_completion_and_operand_roots();
    }
}

#[cfg_attr(test, test)]
fn staged_async_generator_refusals_preserve_awaited_values_and_delegation() {
    let prefix =
        "Promise; TypeError; globalThis.sentinel = {}; globalThis.payload = 'x'.repeat(4096);";
    for (setup, operation) in [
        (
            "globalThis.g = (async function*() {try {yield 1;} finally {yield 2;}})(); g.next();",
            "(async () => {var first = await g.return(Promise.resolve(payload)); var last = await g.next(); return first.value === 2 && !first.done && last.value === payload && last.done ? 42 : 0;})()",
        ),
        (
            "globalThis.resolve = undefined; globalThis.pending = new Promise(r => {resolve = r;}); globalThis.g = (async function*() {try {yield 1;} finally {yield 2;}})(); g.next(); g.return(pending);",
            "(async () => {resolve(payload); var first = await g.next(); var last = await g.next(); return first.value === payload && first.done && last.done ? 42 : 0;})()",
        ),
        (
            "globalThis.delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {return {done:true,value:payload};}}; globalThis.g = (async function*() {try {yield* delegate;} finally {yield 2;}})(); g.next();",
            "(async () => {var first = await g.return(13); var last = await g.next(); return first.value === 2 && !first.done && last.value === payload && last.done ? 42 : 0;})()",
        ),
        (
            "globalThis.close = Promise.resolve({done:true}); Object.defineProperty(close, 'constructor', {get() {throw sentinel;}}); globalThis.delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {return close;}}; globalThis.g = (async function*() {try {yield* delegate;} catch(error) {if(error !== sentinel) throw error; return 42;}})(); g.next();",
            "(async () => {var result = await g.throw(13); return result.value === 42 && result.done && (await g.next()).done ? 42 : 0;})()",
        ),
        (
            "globalThis.delegate = {[Symbol.iterator]() {return this;}, next() {return {done:false,value:7};}, return() {throw new RangeError('close');}}; globalThis.g = (async function*() {try {yield* delegate;} catch(error) {if(!(error instanceof RangeError)) throw error; return 42;}})(); g.next();",
            "(async () => {var result = await g.throw(13); return result.value === 42 && result.done ? 42 : 0;})()",
        ),
        (
            "globalThis.delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, get return() {null.value;}}; globalThis.g = (async function*() {try {yield* delegate;} catch(error) {if(!(error instanceof TypeError)) throw error; return 42;}})(); g.next();",
            "(async () => {var result = await g.return(13); return result.value === 42 && result.done ? 42 : 0;})()",
        ),
        (
            "globalThis.delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {null.value;}}; globalThis.g = (async function*() {try {yield* delegate;} catch(error) {if(!(error instanceof TypeError)) throw error; return 42;}})(); g.next();",
            "(async () => {var result = await g.throw(13); return result.value === 42 && result.done ? 42 : 0;})()",
        ),
        (
            "globalThis.delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {return {get done() {null.value;}};}}; globalThis.g = (async function*() {try {yield* delegate;} catch(error) {if(!(error instanceof TypeError)) throw error; return 42;}})(); g.next();",
            "(async () => {var result = await g.return(13); return result.value === 42 && result.done ? 42 : 0;})()",
        ),
        (
            "globalThis.delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {return {done:true,value:payload};}}; globalThis.g = (async function*() {try {yield* delegate;} finally {null.value;}})(); g.next();",
            "(async () => {try {await g.return(13);} catch(error) {return error instanceof TypeError && (await g.next()).done ? 42 : 0;} return 0;})()",
        ),
        (
            "globalThis.pending = Promise.withResolvers(); globalThis.g = (async function*() {yield pending.promise;})(); g.next();",
            "(async () => {pending.resolve(payload); var result = await g.next(); return result.done ? 42 : 0;})()",
        ),
    ] {
        let setup = compile(&parse(&format!("{prefix}{setup}")).unwrap()).unwrap();
        Vm::verify_async_allocation_boundary(&setup, operation);
    }
}

#[cfg_attr(test, test)]
fn staged_sync_delegation_refusals_retain_completed_values() {
    for operation in ["g.next()", "g.return(13)", "g.throw(13)"] {
        Vm::verify_script_allocation_boundary(
            "globalThis.payload = 'x'.repeat(4096); globalThis.finished = false; globalThis.delegate = {[Symbol.iterator]() {return this;}, next() {if (finished) return {done:true,value:payload}; finished = true; return {done:false,value:7};}, return() {return {done:true,value:payload};}, throw() {return {done:true,value:payload};}}; globalThis.g = (function*() {return yield* delegate;})(); g.next();",
            &format!("var result = {operation}; result.done && result.value.length === 4096 ? 42 : 0"),
        );
    }
}

#[cfg_attr(test, test)]
fn async_generator_allocation_failures_preserve_queued_completion_and_operand_roots() {
    let setup = compile(
        &parse("Promise; TypeError; globalThis.sentinel = {}; globalThis.payload = 'x'.repeat(512); (async function*() {})();").unwrap(),
    )
    .unwrap();
    for source in [
        "(async () => {var inner = Promise.resolve(42); inner.then = undefined; Object.defineProperty(inner, 'constructor', {get() {throw sentinel;}}); var outer = new Promise(resolve => resolve(inner)); var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (error !== sentinel) throw error; return 42;}})(); await g.next(); var result = await g.return(outer); return result.value === 42 && result.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var g = (async function*() {var nested = (function*() {yield 42;})(); yield nested.next().value;})(); var first = await g.next(); return first.value === 42 && !first.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var source = {[Symbol.iterator]() {return this;}, next() {return {done:false,value:{}};}, return() {return {done:true};}}; var g = (async function*() {var [{x = (function() {var garbage = {}; return 42;})()}] = source; yield x;})(); var first = await g.next(); return first.value === 42 && !first.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (!(error instanceof TypeError)) throw error; return 42;}})(); await g.next(); var result = await g.throw(sentinel); return result.value === 42 && result.done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.iterator]() {return this;}, next() {return {done:false,value:7};}, return() {return 13;}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (!(error instanceof TypeError)) throw error; return 42;}})(); await g.next(); var result = await g.throw(sentinel); return result.value === 42 && result.done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.iterator]() {return this;}, next() {return {done:false,value:7};}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (!(error instanceof TypeError)) throw error; return 42;}})(); await g.next(); var result = await g.throw(sentinel); return result.value === 42 && result.done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, get return() {throw payload;}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (error !== payload) throw error; return 42;}})(); await g.next(); var result = await g.return(13); return result.value === 42 && result.done ? 42 : 0;})()",
        "(async () => {var called = false; var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:called,value:42};}, return() {called = true; return {done:false,value:42};}, throw() {called = true; return {done:false,value:42};}}; var g = (async function*() {return yield* delegate;})(); await g.next(); var result = await g.return(13); return result.value === 42 && !result.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var called = false; var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:called,value:42};}, throw() {called = true; return {done:false,value:42};}}; var g = (async function*() {return yield* delegate;})(); await g.next(); var result = await g.throw(13); return result.value === 42 && !result.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {return {done:true,value:42};}}; var g = (async function*() {try {yield* delegate;} finally {throw payload;}})(); await g.next(); try {await g.return(13);} catch (error) {return error === payload && (await g.next()).done ? 42 : 0;} return 0;})()",
        "(async () => {var g = (async function*() {})(); await g.next(); var result = await g.return(Promise.resolve(payload)); return result.done && result.value === payload && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var g = (async function*() {try {yield 1;} finally {yield 2;}})(); await g.next(); var first = await g.return(Promise.resolve(payload)), second = await g.next(); return first.value === 2 && !first.done && second.value === payload && second.done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {return {done:true,value:payload};}}; var g = (async function*() {try {yield* delegate;} finally {yield 2;}})(); await g.next(); var first = await g.return(13), second = await g.next(); return first.value === 2 && !first.done && second.value === payload && second.done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, throw() {return {done:true,value:payload};}}; var g = (async function*() {var value = yield* delegate; return value === payload ? 42 : 0;})(); await g.next(); var result = await g.throw(13); return result.value === 42 && result.done ? 42 : 0;})()",
        "(async () => {var g = (async function*() {yield 42;})(); var a = await g.next(), b = await g.next(); return a.value === 42 && !a.done && b.done ? 42 : 0;})()",
        "(async () => {var g = (async function*() {yield Promise.resolve(42);})(); var a = await g.next(), b = await g.next(); return a.value === 42 && !a.done && b.done ? 42 : 0;})()",
        "(async () => {var g = (async function*() {yield* [42];})(); var a = await g.next(), b = await g.next(); return a.value === 42 && !a.done && b.done ? 42 : 0;})()",
        "(async () => {var g = (async function*() {yield 1;})(); var results = await Promise.all([g.next(), g.return(Promise.resolve(42)), g.next()]); return results[0].value === 1 && results[1].value === 42 && results[1].done && results[2].done ? 42 : 0;})()",
        "(async () => {var g = (async function*() {try {yield 1;} finally {yield 2;}})(); await g.next(); var a = await g.return(Promise.resolve(sentinel)), b = await g.next(); return a.value === 2 && !a.done && b.value === sentinel && b.done ? 42 : 0;})()",
        "(async () => {var g = (async function*() {try {yield 1;} finally {}})(); await g.next(); try {await g.throw(sentinel);} catch (error) {return error === sentinel && (await g.next()).done ? 42 : 0;} return 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return Promise.resolve({done:false,value:7});}, return(value) {return Promise.resolve({done:true,value});}, throw(error) {throw error;}}; var g = (async function*() {try {yield* delegate;} finally {}})(); await g.next(); var ended = await g.return(42); return ended.value === 42 && ended.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return Promise.resolve({done:false,value:7});}, return() {return Promise.resolve({done:true});}}; var g = (async function*() {try {yield* delegate;} finally {}})(); await g.next(); try {await g.throw(sentinel);} catch (error) {return error instanceof TypeError && (await g.next()).done ? 42 : 0;} return 0;})()",
        "(async () => {var g = (async function*() {try {yield Promise.reject(sentinel);} catch (error) {if (error !== sentinel) throw error; return 42;}})(); var answer = await g.next(); return answer.value === 42 && answer.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var g = (async function*() {yield 1;})(); try {await g.return(Promise.reject(sentinel));} catch (error) {return error === sentinel && (await g.next()).done ? 42 : 0;} return 0;})()",
        "(async () => {var g = (async function*() {})(); await g.next(); try {await g.return(Promise.reject(sentinel));} catch (error) {return error === sentinel && (await g.next()).done ? 42 : 0;} return 0;})()",
        "(async () => {var g = (async function*() {})(); await g.next(); var value = Promise.resolve(42); Object.defineProperty(value, 'constructor', {get() {throw sentinel;}}); try {await g.return(value);} catch (error) {return error === sentinel && (await g.next()).done ? 42 : 0;} return 0;})()",
        "(async () => {var reads = 0, value = {get then() {if (reads++ === 0) return undefined; throw sentinel;}}; var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (error !== sentinel) throw error; return 42;}})(); await g.next(); var answer = await g.return(value); return reads === 2 && answer.value === 42 && answer.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, get throw() {throw sentinel;}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (error !== sentinel) throw error; return 42;}})(); await g.next(); var answer = await g.throw(13); return answer.value === 42 && answer.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {throw sentinel;}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (error !== sentinel) throw error; return 42;}})(); await g.next(); var answer = await g.return(13); return answer.value === 42 && answer.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {return 13;}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (!(error instanceof TypeError)) throw error; return 42;}})(); await g.next(); var answer = await g.return(19); return answer.value === 42 && answer.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.iterator]() {return this;}, next() {return {done:false,value:7};}, return() {return 13;}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (!(error instanceof TypeError)) throw error; return 42;}})(); await g.next(); var answer = await g.return(19); return answer.value === 42 && answer.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.iterator]() {return this;}, next() {return {done:false,value:7};}, get throw() {throw sentinel;}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (error !== sentinel) throw error; return 42;}})(); await g.next(); var answer = await g.throw(13); return answer.value === 42 && answer.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, return() {throw sentinel;}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (error !== sentinel) throw error; return 42;}})(); await g.next(); var answer = await g.throw(13); return answer.value === 42 && answer.done && (await g.next()).done ? 42 : 0;})()",
        "(async () => {var delegate = {[Symbol.asyncIterator]() {return this;}, next() {return {done:false,value:7};}, throw() {throw sentinel;}}; var g = (async function*() {try {yield* delegate;} catch (error) {if (error !== sentinel) throw error;}})(); await g.next(); var answer = await g.throw(13); return answer.done && answer.value === undefined ? 42 : 0;})()",
        "(async () => {var run = (function() {let captured = 7; return async function() {eval('var captured = 11'); captured += payload; return captured.length === payload.length + 2 ? 42 : 0;};})(); return await run();})()",
        "(async () => {var run = (function() {let captured = 7; return async function() {eval('var captured = 11'); captured = payload; return captured === payload ? 42 : 0;};})(); return await run();})()",
    ] {
        Vm::verify_async_allocation_boundary(&setup, source);
    }
}
