// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! State-record contracts at the safe-integer collection boundary.

use super::*;
use crate::{compile, parse};

#[cfg_attr(test, test)]
fn maximum_collection_length_closes_without_calling_next() {
    let mut vm = Vm::default();
    let iterator = vm
        .execute_script(
            &compile(
                &parse(
                    r#"
        globalThis.nextCalls = 0; globalThis.closeCalls = 0;
        ({[Symbol.asyncIterator]() {return this;},
          next() {nextCalls++; throw 'next must not run';},
          return() {closeCalls++; return Promise.resolve({done:true});}})
    "#,
                )
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let method = vm
        .get_method(&iterator, &JsSymbol::well_known("asyncIterator").into())
        .unwrap();
    let record = vm
        .async_iterator_record_from_method(&iterator, method)
        .unwrap();
    let record_root = vm.heap.root(record.object_id().unwrap()).unwrap();
    let state = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let root = vm.heap.root(state).unwrap();
    let state = vm.fa_state(state).unwrap();
    let promise = vm.new_promise().unwrap();
    vm.fa_set(state, "promise", Value::Object(promise)).unwrap();
    vm.fa_set(state, "record", record).unwrap();
    vm.fa_set(state, "k", Value::Number(MAX_SAFE_INTEGER))
        .unwrap();
    let limit = vm.heap.allow_only(0);
    assert_eq!(
        vm.fa_iterator_next(state),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit }))
    );
    assert!(vm.stack.is_empty());
    vm.heap.allow_only(16 * 1024 * 1024);
    vm.fa_iterator_next(state).unwrap();
    vm.run_promise_jobs().unwrap();
    let PromiseStatus::Rejected(error) = &vm.promises[&promise].status else {
        panic!("oversized collection did not reject")
    };
    let error = error.clone();
    assert_eq!(
        vm.get_property(&error, &"name".into()).unwrap(),
        Value::String("TypeError".into())
    );
    assert_eq!(
        vm.lookup_global_name("nextCalls").unwrap(),
        Some(Value::Number(0.0))
    );
    assert_eq!(
        vm.lookup_global_name("closeCalls").unwrap(),
        Some(Value::Number(1.0))
    );
    let second = vm.new_promise().unwrap();
    vm.fa_set(state, "promise", Value::Object(second)).unwrap();
    vm.fa_close_and_reject(state, Value::Number(7.0)).unwrap();
    assert!(matches!(
        vm.promises[&second].status,
        PromiseStatus::Rejected(Value::Number(7.0))
    ));
    assert_eq!(
        vm.lookup_global_name("closeCalls").unwrap(),
        Some(Value::Number(1.0))
    );
    vm.heap.unroot(root).unwrap();
    vm.heap.unroot(record_root).unwrap();
}

#[cfg_attr(test, test)]
fn absent_state_fields_have_explicit_defaults_and_missing_capabilities_reject() {
    let mut vm = Vm::default();
    let state = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    let root = vm.heap.root(state).unwrap();
    let state = vm.fa_state(state).unwrap();
    assert_eq!(vm.fa_get(state, "absent"), Value::Undefined);
    assert_eq!(vm.fa_number(state, "absent"), 0.0);
    assert_eq!(
        vm.fa_promise(state),
        Err(RuntimeError::TypeError(
            "fromAsync state lost its promise".into()
        ))
    );
    // An empty state has no iterator record to mark. A produced record owns
    // the existing Boolean field, so marking it must also work at zero room.
    vm.fa_mark_done(state);
    let iterable = vm
        .execute_script(&compile(&parse("[]").unwrap()).unwrap())
        .unwrap();
    let record = vm.get_iterator(&iterable).unwrap();
    let record_id = record.object_id().unwrap();
    vm.fa_set(state, "record", record).unwrap();
    vm.heap.allow_only(0);
    vm.fa_mark_done(state);
    assert_eq!(
        vm.heap.get_own(record_id, "done").unwrap(),
        Some(Value::Bool(true))
    );
    vm.heap.allow_only(16 * 1024 * 1024);
    let base = vm.stack.len();
    assert_eq!(
        vm.fa_reject(state, Value::Number(7.0)),
        Err(RuntimeError::TypeError(
            "fromAsync state lost its promise".into()
        ))
    );
    assert_eq!(vm.stack.len(), base);
    assert_eq!(
        vm.fa_resolve(state, Value::Number(42.0)),
        Err(RuntimeError::TypeError(
            "fromAsync state lost its promise".into()
        ))
    );
    assert_eq!(vm.stack.len(), base);
    vm.heap.unroot(root).unwrap();
}

#[cfg_attr(test, test)]
fn state_operations_reject_an_object_from_another_heap() {
    let mut owner = Vm::default();
    let state = owner.heap.alloc_object(None).unwrap();
    let root = owner.heap.root(state).unwrap();
    let mut vm = Vm::default();
    let expected = Err(RuntimeError::Heap(HeapError::InvalidObject(state)));
    assert!(
        matches!(vm.fa_state(state), Err(RuntimeError::Heap(HeapError::InvalidObject(actual))) if actual == state)
    );
    assert_eq!(
        vm.array_from_async_resume(state, Value::Undefined, false),
        expected
    );
    assert!(vm.stack.is_empty());
    assert!(owner.heap.contains(state));
    owner.heap.unroot(root).unwrap();
}

impl Vm {
    /// Runs state-record contracts only in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_array_from_async_boundary_contracts() {
        maximum_collection_length_closes_without_calling_next();
        absent_state_fields_have_explicit_defaults_and_missing_capabilities_reject();
        state_operations_reject_an_object_from_another_heap();
        from_async_allocation_failures_restore_roots_across_await_and_close_turns();
    }
}

#[cfg_attr(test, test)]
fn from_async_allocation_failures_restore_roots_across_await_and_close_turns() {
    let setup = compile(&parse("Array.fromAsync; Promise; Object.preventExtensions; TypeError; globalThis.sentinel = {};").unwrap()).unwrap();
    for source in [
        "Array.fromAsync([1,2]).then(a => a.join() === '1,2' ? 42 : 0)",
        "Array.fromAsync({length:2,0:Promise.resolve(1),1:2}, v => Promise.resolve(v + 1)).then(a => a.join() === '2,3' ? 42 : 0)",
        "Array.fromAsync([1], () => {throw sentinel;}).then(() => 0, error => error === sentinel ? 42 : 0)",
        "Array.fromAsync({[Symbol.asyncIterator]() {return this;}, next() {return Promise.resolve({done:false,value:1});}, return() {return Promise.resolve({done:true});}}, () => Promise.reject(sentinel)).then(() => 0, error => error === sentinel ? 42 : 0)",
        "Array.fromAsync({length:1,get 0() {throw sentinel;}}).then(() => 0, error => error === sentinel ? 42 : 0)",
        "Array.fromAsync({[Symbol.asyncIterator]() {return this;}, next() {return Promise.reject(sentinel);}}).then(() => 0, error => error === sentinel ? 42 : 0)",
        "Array.fromAsync({[Symbol.asyncIterator]() {return this;}, next() {return 7;}}).then(() => 0, error => error instanceof TypeError ? 42 : 0)",
        "Array.fromAsync.call(function() {return Object.preventExtensions({length:0});}, [1]).then(() => 0, error => error instanceof TypeError ? 42 : 0)",
    ] {
        Vm::verify_async_allocation_boundary(&setup, source);
    }
}

impl Vm {
    pub(in crate::vm) fn verify_async_allocation_boundary(setup: &Bytecode, source: &str) {
        let reuse = compile(&parse("21 + 21").unwrap()).unwrap();
        let operation = compile(&parse(source).unwrap()).unwrap();
        let mut failures = 0;
        let mut completed = false;
        let mut next_extra = 0;
        while next_extra <= 16 * 1024 * 1024 {
            let extra = next_extra;
            let mut vm = Vm::default();
            vm.execute_script(setup).unwrap();
            // A staged fixture may leave a generator at an awaited/yielded
            // boundary. Finish the setup jobs before applying its byte budget.
            vm.run_promise_jobs().unwrap();
            vm.execute_script(&reuse).unwrap();
            vm.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })
            .unwrap();
            let limit = vm.heap.allow_only(extra);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                vm.execute_script(&operation).and_then(|promise| {
                    vm.run_promise_jobs()?;
                    let id = promise.object_id().expect("the fixture returns a Promise");
                    assert!(matches!(&vm.promises[&id].status, PromiseStatus::Fulfilled(Value::Number(answer)) if *answer == 42.0), "{source}, {extra}: unexpected settled result");
                    Ok(())
                })
            }))
            .unwrap_or_else(|_| panic!("async allocation panic, headroom {extra}: {source}"));
            match result {
                Ok(()) => completed = true,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: actual })) => {
                    assert_eq!(actual, limit);
                    failures += 1;
                }
                Err(error) => panic!("{source}, headroom {extra}: {error:?}"),
            }
            assert!(
                vm.stack.is_empty(),
                "{source}, {extra}: failed await retained operand roots"
            );
            assert!(
                vm.async_frame_roots.is_empty(),
                "{source}, {extra}: active async roots outlived their job"
            );
            if !completed {
                next_extra = vm.heap.next_allocation_headroom(extra);
            }
            vm.heap.allow_only(16 * 1024 * 1024);
            assert_eq!(
                vm.execute_script(&reuse),
                Ok(Value::Number(42.0)),
                "{source}, {extra}"
            );
            if completed {
                break;
            }
        }
        assert!(
            completed && failures > 0,
            "no allocation boundary was reached: {source}"
        );
        // Sweep root registrations separately from bytes; the prepared
        // asynchronous source and its exact successful result are identical.
        let mut completed = false;
        let mut refused = 0;
        for registrations in 0..=256 {
            let mut vm = Vm::default();
            vm.execute_script(setup).unwrap();
            vm.run_promise_jobs().unwrap();
            vm.execute_script(&reuse).unwrap();
            vm.heap.allow_root_registrations(registrations);
            let result = vm.execute_script(&operation).and_then(|promise| {
                vm.run_promise_jobs()?;
                let id = promise.object_id().expect("the fixture returns a Promise");
                assert!(
                    matches!(&vm.promises[&id].status,
                    PromiseStatus::Fulfilled(Value::Number(answer)) if *answer == 42.0),
                    "{source}, remaining roots {registrations}: unexpected settlement"
                );
                Ok(())
            });
            match result {
                Ok(()) => completed = true,
                Err(RuntimeError::Heap(HeapError::IdExhausted)) => refused += 1,
                Err(error) => panic!("{source}, remaining roots {registrations}: {error:?}"),
            }
            assert!(
                vm.stack.is_empty(),
                "{source}, remaining roots {registrations}"
            );
            assert!(vm.async_frame_roots.iter().all(|id| vm.heap.contains(*id)));
            assert_eq!(vm.execute_script(&reuse), Ok(Value::Number(42.0)));
            if completed {
                break;
            }
        }
        assert!(
            completed && refused > 0,
            "{source} did not exercise root-registration refusal"
        );
    }
}
