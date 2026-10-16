// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny callback inputs keep one owner during task/frame handoff.

use super::*;

fn execute(vm: &mut Vm, source: &str) {
    let code = crate::compile(&crate::parse(source).unwrap()).unwrap();
    vm.execute_script(&code).unwrap();
}

fn observed_vm() -> (Vm, Rc<RefCell<Vec<usize>>>) {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    let account = vm.host_scheduling.as_ref().unwrap().account.clone();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let output = observed.clone();
    vm.install_host_function("observe", 0, move |_args: &[HostValue]| {
        output
            .borrow_mut()
            .push(account.borrow().usage.payload_bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    (vm, observed)
}

fn payload_difference(plain: &[usize], payload: &[usize]) -> Vec<usize> {
    assert_eq!(plain.len(), 2);
    assert_eq!(payload.len(), 2);
    plain
        .iter()
        .zip(payload)
        .map(|(plain, payload)| payload.checked_sub(*plain).unwrap())
        .collect()
}

#[cfg_attr(test, test)]
fn timer_payload_handoff_charges_one_frame_copy_and_releases_it_before_jobs() {
    for (repeat, clear, throws, parameter, expected) in [
        (false, false, false, false, vec![14, 0]),
        (false, false, true, false, vec![14, 0]),
        (true, false, false, false, vec![28, 14]),
        (true, true, false, false, vec![14, 0]),
        (true, true, true, false, vec![14, 0]),
        (false, false, false, true, vec![28, 0]),
        (true, false, false, true, vec![42, 14]),
    ] {
        let mut runs = Vec::new();
        for argument in ["0", "'payload'"] {
            let (mut vm, observed) = observed_vm();
            vm.install_host_timer_functions().unwrap();
            let cancel = if clear { "clearInterval(id);" } else { "" };
            let throw = if throws { "throw 7;" } else { "" };
            let timer = if repeat { "setInterval" } else { "setTimeout" };
            let parameter = if parameter { "value" } else { "" };
            execute(&mut vm, &format!("function callback({parameter}) {{ {cancel} observe(); Promise.resolve().then(() => observe()); {throw} }} var id = {timer}(callback, 1, {argument}); 0;"));
            let run = vm.run_due_host_timers(1).unwrap();
            assert_eq!(run.tasks_run, 1);
            assert_eq!(run.uncaught_errors, u32::from(throws));
            runs.push(observed.borrow().clone());
            execute(&mut vm, "clearInterval(id); 0;");
            assert_eq!(vm.displaced_execution_bytes.get(), 0);
        }
        assert_eq!(
            payload_difference(&runs[0], &runs[1]),
            expected,
            "repeat={repeat}, clear={clear}, throws={throws}: {runs:?}"
        );
    }
}

#[cfg_attr(test, test)]
fn drag_payload_handoff_transfers_text_to_the_callback_frame() {
    let mut runs = Vec::new();
    let mut input_bytes = 0;
    for text in ["", "payload"] {
        let (mut vm, observed) = observed_vm();
        execute(
            &mut vm,
            "function callback() { observe(); Promise.resolve().then(() => observe()); } 0;",
        );
        let callback = vm
            .lookup_global_name("callback")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        let text: JsString = text.into();
        input_bytes = text.owned_heap_payload_bytes().unwrap();
        vm.queue_drag_string_task(callback, text).unwrap();
        assert!(vm.run_browser_task_turn(1).unwrap());
        runs.push(observed.borrow().clone());
        assert_eq!(vm.displaced_execution_bytes.get(), 0);
    }
    assert_eq!(payload_difference(&runs[0], &runs[1]), vec![input_bytes, 0]);
}

#[cfg_attr(test, test)]
fn finalization_payload_handoff_releases_holdings_before_checkpoint_jobs() {
    for throws in [false, true] {
        let mut runs = Vec::new();
        for holdings in ["0", "'payload'"] {
            let (mut vm, observed) = observed_vm();
            let completion = if throws {
                "throw 7;"
            } else {
                "return 'ignored';"
            };
            execute(&mut vm, &format!("var registry = new FinalizationRegistry(function() {{ observe(); Promise.resolve().then(() => observe()); {completion} }}); registry.register({{}}, {holdings}); 0;"));
            vm.collect_realm_garbage().unwrap();
            assert!(vm.run_browser_task_turn(1).unwrap());
            runs.push(observed.borrow().clone());
            assert_eq!(vm.displaced_execution_bytes.get(), 0);
        }
        assert_eq!(payload_difference(&runs[0], &runs[1]), vec![14, 0]);
    }
}

#[cfg_attr(test, test)]
fn reaction_payload_handoff_preserves_only_live_result_and_frame_owners() {
    for (rejected, parameter, throws, passthrough, expected) in [
        (false, false, false, false, vec![28, 14]),
        (true, false, false, false, vec![28, 14]),
        (false, true, false, false, vec![42, 14]),
        (false, false, true, false, vec![28, 14]),
        (false, false, false, true, vec![42, 28]),
    ] {
        let mut runs = Vec::new();
        for argument in ["0", "'payload'"] {
            let (mut vm, observed) = observed_vm();
            let method = if rejected { "reject" } else { "resolve" };
            let parameter = if parameter { "value" } else { "" };
            let completion = if throws { "throw 0;" } else { "" };
            let input = if passthrough {
                "source.then()"
            } else {
                "source"
            };
            execute(&mut vm, &format!("var source = Promise.{method}({argument}); var input = {input}; function callback({parameter}) {{ observe(); {completion} }} var result = input.then(callback, callback); result.then(() => observe(), () => observe()); 0;"));
            vm.run_promise_jobs().unwrap();
            runs.push(observed.borrow().clone());
            assert_eq!(vm.displaced_execution_bytes.get(), 0);
            assert_eq!(vm.active_scheduling_bytes, 0);
        }
        assert_eq!(payload_difference(&runs[0], &runs[1]), expected, "rejected={rejected}, parameter={parameter}, throws={throws}, passthrough={passthrough}: {runs:?}");
    }
}

#[cfg_attr(test, test)]
fn species_payload_handoff_keeps_gc_roots_without_primitive_copies() {
    for throws in [false, true] {
        let mut runs = Vec::new();
        for argument in ["0", "'payload'"] {
            let (mut vm, observed) = observed_vm();
            let completion = if throws {
                "throw value;"
            } else {
                "return value;"
            };
            execute(&mut vm, &format!("function Species(executor) {{ executor(function(value) {{ observe(); }}, function(value) {{ observe(); }}); }} var source = Promise.resolve({argument}); source.constructor = {{ [Symbol.species]: Species }}; source.then(function(value) {{ observe(); {completion} }}); 0;"));
            vm.run_promise_jobs().unwrap();
            runs.push(observed.borrow().clone());
            assert_eq!(vm.displaced_execution_bytes.get(), 0);
            assert_eq!(vm.active_scheduling_bytes, 0);
        }
        assert_eq!(payload_difference(&runs[0], &runs[1]), vec![42, 42]);
    }
}

#[cfg_attr(test, test)]
fn awaited_payload_handoff_transfers_input_to_the_resumed_frame() {
    for generator in [false, true] {
        let mut runs = Vec::new();
        for argument in ["0", "'payload'"] {
            let (mut vm, observed) = observed_vm();
            let star = if generator { "*" } else { "" };
            let completion = if generator { "yield 0;" } else { "return 0;" };
            let result = if generator {
                "var iterator = f(); iterator.next()"
            } else {
                "f()"
            };
            execute(&mut vm, &format!("var source = Promise.resolve({argument}); async function{star} f() {{ const value = await source; observe(); {completion} }} {result}.then(() => observe()); 0;"));
            vm.run_promise_jobs().unwrap();
            runs.push(observed.borrow().clone());
            assert_eq!(vm.displaced_execution_bytes.get(), 0);
            assert_eq!(vm.active_scheduling_bytes, 0);
        }
        assert_eq!(
            runs[1]
                .iter()
                .zip(&runs[0])
                .map(|(payload, plain)| *payload as isize - *plain as isize)
                .collect::<Vec<_>>(),
            vec![28, 14],
            "generator={generator}: {runs:?}"
        );
    }
}

#[cfg_attr(test, test)]
fn host_completion_payload_handoff_preserves_settled_and_running_owners() {
    for rejected in [false, true] {
        let mut runs = Vec::new();
        for string in [false, true] {
            let (mut vm, observed) = observed_vm();
            let tokens = Rc::new(RefCell::new(Vec::new()));
            let output = tokens.clone();
            vm.install_host_async_function(
                "request",
                0,
                HostTaskSource::Networking,
                move |token: HostPromiseToken, _args: &[HostValue]| {
                    output.borrow_mut().push(token);
                    Ok(())
                },
            )
            .unwrap();
            execute(&mut vm, "var source = request(); function callback() { observe(); } var result = source.then(callback, callback); result.then(() => observe()); 0;");
            let value = if string {
                HostValue::String(JsString::from_code_units(
                    "payload".bytes().map(u16::from).collect(),
                ))
            } else {
                HostValue::Number(0.0)
            };
            let completion = if rejected {
                HostPromiseCompletion::Reject(value)
            } else {
                HostPromiseCompletion::Resolve(value)
            };
            assert!(vm
                .queue_host_promise_completion(tokens.borrow()[0], completion)
                .unwrap());
            assert!(vm.run_browser_task_turn(1).unwrap());
            runs.push(observed.borrow().clone());
            assert_eq!(vm.displaced_execution_bytes.get(), 0);
            assert_eq!(vm.active_scheduling_bytes, 0);
        }
        assert_eq!(payload_difference(&runs[0], &runs[1]), vec![28, 14]);
    }
}

#[cfg_attr(test, test)]
fn generator_yield_payload_handoff_releases_input_before_successor_requests() {
    let mut runs = Vec::new();
    for argument in ["0", "'payload'"] {
        let (mut vm, observed) = observed_vm();
        execute(&mut vm, &format!("var source = Promise.resolve({argument}); async function* f() {{ yield source; observe(); }} var iterator = f(); iterator.next(); iterator.next().then(() => observe()); 0;"));
        vm.run_promise_jobs().unwrap();
        runs.push(observed.borrow().clone());
        assert_eq!(vm.displaced_execution_bytes.get(), 0);
    }
    assert_eq!(payload_difference(&runs[0], &runs[1]), vec![14, 14]);
}

#[cfg_attr(test, test)]
fn generator_delegate_payload_handoff_discards_invalid_results_before_resuming() {
    let mut runs = Vec::new();
    for argument in ["0", "'payload'"] {
        let (mut vm, observed) = observed_vm();
        execute(&mut vm, &format!("var source = Promise.resolve({argument}); async function* f() {{ try {{ yield* {{ [Symbol.asyncIterator]() {{ return {{ next() {{ return source; }} }}; }} }}; }} catch {{ observe(); }} }} f().next().then(() => observe()); 0;"));
        vm.run_promise_jobs().unwrap();
        runs.push(observed.borrow().clone());
        assert_eq!(vm.displaced_execution_bytes.get(), 0);
    }
    assert_eq!(payload_difference(&runs[0], &runs[1]), vec![14, 14]);
}

#[cfg_attr(test, test)]
fn legacy_cleanup_payload_handoff_transfers_holdings_into_the_callback() {
    let mut runs = Vec::new();
    for string in [false, true] {
        let (mut vm, observed) = observed_vm();
        execute(
            &mut vm,
            "function callback() { observe(); Promise.resolve().then(() => observe()); } 0;",
        );
        let callback = vm.lookup_global_name("callback").unwrap().unwrap();
        let holdings = if string {
            Value::String(JsString::from_code_units(
                "payload".bytes().map(u16::from).collect(),
            ))
        } else {
            Value::Number(0.0)
        };
        vm.enqueue_promise_job(PromiseJob::FinalizationCleanup { callback, holdings });
        vm.run_promise_jobs().unwrap();
        runs.push(observed.borrow().clone());
        assert_eq!(vm.displaced_execution_bytes.get(), 0);
    }
    assert_eq!(payload_difference(&runs[0], &runs[1]), vec![14, 0]);
}

#[cfg_attr(test, test)]
fn import_payload_handoff_keeps_borrowed_loader_strings_through_module_execution() {
    let (mut vm, observed) = observed_vm();
    let counter = vm.displaced_execution_bytes.clone();
    let output = observed.clone();
    vm.install_host_function("observeLease", 0, move |_args: &[HostValue]| {
        output.borrow_mut().push(counter.get());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let code = crate::compile_module(
        &crate::parse_module("observeLease(); export const value = 0;").unwrap(),
    )
    .unwrap();
    vm.set_module_loader_context(
        "dir/main.js",
        HashMap::from([("dir/entry.js".into(), code)]),
    );
    execute(&mut vm, "import('./entry.js'); 0;");
    let expected = match vm.promise_jobs.front().unwrap() {
        PromiseJob::DynamicImport {
            referrer,
            specifier,
            ..
        } => referrer.capacity() + specifier.capacity(),
        _ => panic!("the import must enqueue its real loader job"),
    };
    vm.run_promise_jobs().unwrap();
    assert_eq!(*observed.borrow(), vec![expected]);
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
}

#[cfg_attr(test, test)]
fn module_await_payload_handoff_transfers_exported_values_into_managed_cells() {
    let mut runs = Vec::new();
    for argument in ["0", "'payload'"] {
        let (mut vm, observed) = observed_vm();
        let code = crate::compile_module(
            &crate::parse_module("export const value = await globalThis.source; observe();")
                .unwrap(),
        )
        .unwrap();
        vm.set_module_loader_context(
            "dir/main.js",
            HashMap::from([("dir/entry.js".into(), code)]),
        );
        execute(&mut vm, &format!("var source = Promise.resolve({argument}); import('./entry.js').then(() => observe()); 0;"));
        vm.run_promise_jobs().unwrap();
        runs.push(observed.borrow().clone());
        assert_eq!(vm.displaced_execution_bytes.get(), 0);
    }
    assert_eq!(payload_difference(&runs[0], &runs[1]), vec![14, 14]);
}

impl crate::Vm {
    /// Runs the task/frame ownership contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_task_payload_handoff_contracts() {
        timer_payload_handoff_charges_one_frame_copy_and_releases_it_before_jobs();
        drag_payload_handoff_transfers_text_to_the_callback_frame();
        finalization_payload_handoff_releases_holdings_before_checkpoint_jobs();
        reaction_payload_handoff_preserves_only_live_result_and_frame_owners();
        species_payload_handoff_keeps_gc_roots_without_primitive_copies();
        awaited_payload_handoff_transfers_input_to_the_resumed_frame();
        host_completion_payload_handoff_preserves_settled_and_running_owners();
        generator_yield_payload_handoff_releases_input_before_successor_requests();
        generator_delegate_payload_handoff_discards_invalid_results_before_resuming();
        legacy_cleanup_payload_handoff_transfers_holdings_into_the_callback();
        import_payload_handoff_keeps_borrowed_loader_strings_through_module_execution();
        module_await_payload_handoff_transfers_exported_values_into_managed_cells();
    }
}
