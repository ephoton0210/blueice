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

impl crate::Vm {
    /// Runs the task/frame ownership contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_task_payload_handoff_contracts() {
        timer_payload_handoff_charges_one_frame_copy_and_releases_it_before_jobs();
        drag_payload_handoff_transfers_text_to_the_callback_frame();
        finalization_payload_handoff_releases_holdings_before_checkpoint_jobs();
    }
}
