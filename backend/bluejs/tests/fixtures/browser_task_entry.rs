// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite in-memory tasks and scalar native allowances exercise entry.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

type Calls = Rc<RefCell<Vec<HostValue>>>;
fn page_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn drag_fixture() -> (Vm, Calls) {
    let mut vm = page_vm();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = calls.clone();
    vm.install_host_function("callback", 1, move |args: &[HostValue]| {
        output.borrow_mut().push(args[0].clone());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm
        .lookup_global_name("callback")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    vm.queue_drag_string_task(callback, "tiny".into()).unwrap();
    vm.remaining_instructions = 73;
    (vm, calls)
}
fn completion_fixture() -> (Vm, HostPromiseToken, ObjectId) {
    let mut vm = page_vm();
    let saved = Rc::new(RefCell::new(None));
    let output = saved.clone();
    vm.install_host_async_function(
        "wait",
        0,
        HostTaskSource::UserInteraction,
        move |token, _args: &[HostValue]| {
            *output.borrow_mut() = Some(token);
            Ok(())
        },
    )
    .unwrap();
    let promise = vm
        .host_async_function_call(0, Value::Undefined, &[], false)
        .unwrap()
        .object_id()
        .unwrap();
    let token = saved.borrow().unwrap();
    vm.queue_host_promise_completion(
        token,
        HostPromiseCompletion::Resolve(HostValue::Number(7.0)),
    )
    .unwrap();
    vm.remaining_instructions = 73;
    (vm, token, promise)
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}

#[cfg_attr(test, test)]
fn browser_task_entry_zero_fuel_preserves_drag_task_root_and_context() {
    let (mut vm, calls) = drag_fixture();
    let roots = vm.heap.root_storage_state();
    vm.config.instruction_budget = 0;
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn browser_task_entry_argument_refusal_preserves_drag_task_root_and_context() {
    let (mut vm, calls) = drag_fixture();
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, std::mem::size_of::<Value>() - 1);
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert!(calls.borrow().is_empty());
}
#[cfg_attr(test, test)]
fn browser_task_entry_zero_fuel_preserves_completion_task_wait_and_status() {
    let (mut vm, token, promise) = completion_fixture();
    let roots = vm.heap.root_storage_state();
    vm.config.instruction_budget = 0;
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.host_promises.pending.len(), 1);
    assert!(vm.host_completion_queued_for_test(token));
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
}
#[cfg_attr(test, test)]
fn browser_task_entry_public_turn_zero_fuel_keeps_queued_drag_delivery() {
    let (mut vm, calls) = drag_fixture();
    let roots = vm.heap.root_storage_state();
    vm.config.instruction_budget = 0;
    assert!(matches!(
        vm.run_host_task_turn(0, 1),
        Err(RuntimeError::InstructionLimit)
    ));
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(calls.borrow().is_empty());
}
use crate::vm::browser_tasks::BrowserTask;
fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn serial(vm: &Vm) -> u64 {
    vm.host_scheduling.as_ref().unwrap().task_serial_state()
}
#[derive(Clone, Copy, Debug)]
enum Kind {
    Drag,
    Completion,
    Read,
    Timeout,
    Rejection,
}
const KINDS: [Kind; 5] = [
    Kind::Drag,
    Kind::Completion,
    Kind::Read,
    Kind::Timeout,
    Kind::Rejection,
];
struct Work {
    vm: Vm,
    kind: Kind,
    token: Option<HostPromiseToken>,
    object: Option<ObjectId>,
}
impl Work {
    fn new(kind: Kind) -> Self {
        let (mut vm, token, object) = match kind {
            Kind::Drag => {
                let (vm, _) = drag_fixture();
                (vm, None, None)
            }
            Kind::Completion => {
                let (vm, token, promise) = completion_fixture();
                (vm, Some(token), Some(promise))
            }
            Kind::Read => {
                let mut vm = page_vm();
                vm.install_web_file_api().unwrap();
                execute(
                    &mut vm,
                    "var reader=new FileReader();reader.readAsText(new Blob(['x']));0;",
                );
                let object = vm
                    .lookup_global_name("reader")
                    .unwrap()
                    .unwrap()
                    .object_id();
                (vm, None, object)
            }
            Kind::Timeout => {
                let mut vm = page_vm();
                vm.install_web_file_api().unwrap();
                let signal = vm
                    .abort_signal_timeout(&[Value::Number(0.0)])
                    .unwrap()
                    .object_id()
                    .unwrap();
                vm.heap.root(signal).unwrap();
                (vm, None, Some(signal))
            }
            Kind::Rejection => {
                let mut vm = page_vm();
                vm.install_web_file_api().unwrap();
                vm.install_host_console().unwrap();
                vm.install_host_error_reporting().unwrap();
                vm.install_host_rejection_tracking().unwrap();
                execute(&mut vm,"onunhandledrejection=e=>e.preventDefault();globalThis.p=Promise.reject('tiny');0;");
                let promise = vm
                    .lookup_global_name("p")
                    .unwrap()
                    .unwrap()
                    .object_id()
                    .unwrap();
                vm.notify_host_rejections().unwrap();
                (vm, None, Some(promise))
            }
        };
        vm.with_roots(|heap| heap.try_collect_major()).unwrap();
        vm.remaining_instructions = 73;
        assert_eq!(vm.browser_tasks.len(), 1);
        Self {
            vm,
            kind,
            token,
            object,
        }
    }
    fn assert_queued_state(&self) {
        assert_eq!(self.vm.browser_tasks.len(), 1);
        match self.kind {
            Kind::Completion => {
                assert_eq!(self.vm.host_promises.pending.len(), 1);
                assert!(self.vm.host_completion_queued_for_test(self.token.unwrap()));
                assert!(matches!(
                    self.vm.promises[&self.object.unwrap()].status,
                    PromiseStatus::Pending
                ));
            }
            Kind::Read => {
                let state = self.vm.heap.file_reader(self.object.unwrap()).unwrap();
                assert_eq!((state.ready, state.epoch, state.loaded), (1, 1, 0));
            }
            Kind::Timeout => assert!(!self
                .vm
                .heap
                .abort_signal(self.object.unwrap())
                .unwrap()
                .aborted()),
            Kind::Rejection => {
                assert!(!self.vm.promises[&self.object.unwrap()].rejection_notified);
                assert_eq!(self.vm.host_rejections.as_ref().unwrap().active, 0);
            }
            _ => {}
        }
    }
}

#[cfg_attr(test, test)]
fn browser_task_entry_zero_fuel_preserves_every_active_task_kind() {
    for kind in KINDS {
        let mut work = Work::new(kind);
        let roots = work.vm.heap.root_storage_state();
        let next = serial(&work.vm);
        work.vm.config.instruction_budget = 0;
        assert_eq!(
            work.vm.run_own_browser_task(0),
            Err(RuntimeError::InstructionLimit),
            "{kind:?}"
        );
        work.assert_queued_state();
        assert_eq!(work.vm.heap.root_storage_state(), roots);
        assert_eq!(work.vm.remaining_instructions, 73);
        assert_eq!(serial(&work.vm), next);
    }
}
#[cfg_attr(test, test)]
fn browser_task_entry_zero_fuel_precedes_native_admission_for_every_active_kind() {
    for kind in KINDS {
        let mut work = Work::new(kind);
        work.vm.config.instruction_budget = 0;
        let _peer = leave_room(&mut work.vm, 0);
        assert_eq!(
            work.vm.run_own_browser_task(0),
            Err(RuntimeError::InstructionLimit),
            "{kind:?}"
        );
        work.assert_queued_state();
        assert_eq!(work.vm.check_scheduling_failure(), Ok(()));
        assert_eq!(work.vm.remaining_instructions, 73);
    }
}
#[cfg_attr(test, test)]
fn browser_task_entry_non_drag_preparation_needs_no_argument_allocation() {
    for kind in [Kind::Completion, Kind::Read, Kind::Timeout] {
        let mut work = Work::new(kind);
        let roots = work.vm.heap.root_storage_state();
        let _peer = leave_room(&mut work.vm, 0);
        let prepared = work.vm.prepare_browser_task_entry(0).unwrap();
        assert!(matches!(prepared.action, BrowserEntryAction::Run));
        assert!(prepared.arguments.is_none());
        work.assert_queued_state();
        assert_eq!(work.vm.heap.root_storage_state(), roots);
        assert_eq!(work.vm.remaining_instructions, 73);
    }
}
#[cfg_attr(test, test)]
fn browser_task_entry_terminal_refusal_preserves_every_active_task_kind() {
    for kind in KINDS {
        let mut work = Work::new(kind);
        let roots = work.vm.heap.root_storage_state();
        work.vm.host_scheduling.as_ref().unwrap().fail();
        assert_eq!(
            work.vm.run_own_browser_task(0),
            Err(RuntimeError::SchedulingLimit)
        );
        if matches!(kind, Kind::Completion) {
            // A failed owner retires its external request instead of keeping
            // the completion queued for a document that can no longer run it.
            assert!(work.vm.browser_tasks.is_empty());
            assert!(work.vm.host_promises.pending.is_empty());
            // Retirement releases the request's one root; capacity is kept.
            let after = work.vm.heap.root_storage_state();
            assert_eq!((after.0 + 1, after.1), (roots.0, roots.1));
        } else {
            work.assert_queued_state();
            assert_eq!(work.vm.heap.root_storage_state(), roots);
        }
        assert_eq!(work.vm.remaining_instructions, 73);
    }
}
#[cfg_attr(test, test)]
fn browser_task_entry_drag_argument_exact_allowance_keeps_text_in_the_queue() {
    let (mut vm, calls) = drag_fixture();
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, std::mem::size_of::<Value>());
    let prepared = vm.prepare_browser_task_entry(0).unwrap();
    let arguments = prepared.arguments.as_ref().unwrap();
    assert!(arguments.values.is_empty());
    assert_eq!(
        host_scheduling::slots(&arguments.values),
        std::mem::size_of::<Value>()
    );
    let BrowserTask::DragString { text, .. } = &vm.browser_tasks[0] else {
        panic!("drag task")
    };
    assert_eq!(text, &JsString::from("tiny"));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(calls.borrow().is_empty());
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
    drop(prepared);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - std::mem::size_of::<Value>()
    );
}
#[cfg_attr(test, test)]
fn browser_task_entry_rust_held_arguments_keep_their_lease_after_vm_drop() {
    let (mut vm, _) = drag_fixture();
    let observed = observer(&vm);
    let prepared = vm.prepare_browser_task_entry(0).unwrap();
    drop(vm);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        std::mem::size_of::<Value>()
    );
    drop(prepared);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn browser_task_entry_dropped_preparation_preserves_all_queue_owners() {
    let (mut vm, calls) = drag_fixture();
    let roots = vm.heap.root_storage_state();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    drop(vm.prepare_browser_task_entry(0).unwrap());
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
    assert!(calls.borrow().is_empty());
    assert_eq!(vm.remaining_instructions, 73);
}
#[cfg_attr(test, test)]
fn browser_task_entry_drag_zero_fuel_can_retry_the_same_delivery() {
    let (mut vm, calls) = drag_fixture();
    let budget = vm.config.instruction_budget;
    vm.config.instruction_budget = 0;
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::InstructionLimit)
    );
    vm.config.instruction_budget = budget;
    vm.run_own_browser_task(0).unwrap();
    assert_eq!(*calls.borrow(), [HostValue::String("tiny".into())]);
    assert!(vm.browser_tasks.is_empty());
}
#[cfg_attr(test, test)]
fn browser_task_entry_completion_retry_spends_one_host_instruction_before_settlement() {
    let (mut vm, token, promise) = completion_fixture();
    vm.config.instruction_budget = 0;
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::InstructionLimit)
    );
    assert!(vm.host_completion_queued_for_test(token));
    vm.config.instruction_budget = 1;
    vm.run_own_browser_task(0).unwrap();
    assert!(vm.browser_tasks.is_empty());
    assert!(vm.host_promises.pending.is_empty());
    assert!(
        matches!(vm.promises[&promise].status,PromiseStatus::Fulfilled(Value::Number(value)) if value==7.0)
    );
}
#[cfg_attr(test, test)]
fn browser_task_entry_committed_drag_dispatch_failure_releases_its_root_and_payload() {
    let (mut vm, calls) = drag_fixture();
    let roots = vm.heap.root_storage_state().0;
    vm.config.instruction_budget = 1;
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::InstructionLimit)
    );
    assert!(vm.browser_tasks.is_empty());
    assert_eq!(vm.browser_tasks.capacity(), 0);
    assert_eq!(vm.heap.root_storage_state().0, roots - 1);
    assert!(calls.borrow().is_empty());
    assert_eq!(vm.active_scheduling_bytes, 0);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn browser_task_entry_drag_text_and_argument_capacity_transfer_once_to_dispatch() {
    let mut vm = page_vm();
    let native = observer(&vm);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    vm.install_host_function("callback", 1, move |_args: &[HostValue]| {
        output
            .borrow_mut()
            .push(native.host_scheduling_snapshot().payload_bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    vm.queue_drag_string_task(callback.object_id().unwrap(), "tiny".into())
        .unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    for _ in 0..2 {
        vm.call_native(
            callback.clone(),
            Value::Undefined,
            vec![Value::String("tiny".into())],
            false,
        )
        .unwrap();
    }
    vm.run_own_browser_task(0).unwrap();
    let seen = seen.borrow();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1] - seen[2], 8);
}
#[cfg_attr(test, test)]
fn browser_task_entry_drag_inputs_do_not_remain_charged_during_the_checkpoint() {
    let (mut vm, _) = drag_fixture();
    let native = observer(&vm);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    vm.install_host_function("job", 0, move |_args: &[HostValue]| {
        output
            .borrow_mut()
            .push(native.host_scheduling_snapshot().payload_bytes);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let job = vm.lookup_global_name("job").unwrap().unwrap();
    for _ in 0..2 {
        vm.enqueue_promise_job(PromiseJob::HostMicrotask {
            callback: job.clone(),
        });
        vm.run_host_microtask_checkpoint().unwrap();
    }
    vm.enqueue_promise_job(PromiseJob::HostMicrotask { callback: job });
    vm.run_own_browser_task(0).unwrap();
    let seen = seen.borrow();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1] - seen[2], 8);
}
#[cfg_attr(test, test)]
fn browser_task_entry_future_timeout_preserves_deadline_serial_and_ambient_fuel() {
    let mut vm = page_vm();
    vm.install_web_file_api().unwrap();
    let signal = vm
        .abort_signal_timeout(&[Value::Number(100.0)])
        .unwrap()
        .object_id()
        .unwrap();
    vm.heap.root(signal).unwrap();
    vm.remaining_instructions = 73;
    vm.config.instruction_budget = 0;
    let next = serial(&vm);
    let order = vm.browser_tasks[0].order();
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    vm.run_own_browser_task(0).unwrap();
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.browser_tasks[0].order(), order);
    assert_eq!(serial(&vm), next);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn browser_task_entry_dead_timeout_cleanup_needs_no_instruction_or_argument_storage() {
    let mut vm = page_vm();
    vm.install_web_file_api().unwrap();
    let signal = vm
        .abort_signal_timeout(&[Value::Number(0.0)])
        .unwrap()
        .object_id()
        .unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(signal));
    vm.remaining_instructions = 73;
    vm.config.instruction_budget = 0;
    let next = serial(&vm);
    let _peer = leave_room(&mut vm, 0);
    vm.run_own_browser_task(0).unwrap();
    assert!(vm.browser_tasks.is_empty());
    assert_eq!(vm.remaining_instructions, 73);
    assert_eq!(serial(&vm), next);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn browser_task_entry_aborted_timeout_cleanup_releases_a_retained_task_root() {
    let mut work = Work::new(Kind::Timeout);
    let signal = work.object.unwrap();
    let root = work.vm.heap.root(signal).unwrap();
    let BrowserTask::AbortTimeout {
        root: task_root, ..
    } = &mut work.vm.browser_tasks[0]
    else {
        panic!("timeout")
    };
    *task_root = Some(root);
    let mut state = work.vm.heap.abort_signal(signal).unwrap().clone();
    state.reason = Value::Number(1.0);
    work.vm
        .with_roots(|heap| heap.set_abort_signal(signal, state))
        .unwrap();
    let roots = work.vm.heap.root_storage_state().0;
    work.vm.config.instruction_budget = 0;
    work.vm.run_own_browser_task(0).unwrap();
    assert!(work.vm.browser_tasks.is_empty());
    assert_eq!(work.vm.heap.root_storage_state().0, roots - 1);
    assert_eq!(work.vm.remaining_instructions, 73);
}
#[cfg_attr(test, test)]
fn browser_task_entry_stale_read_cleanup_releases_its_root_without_instruction() {
    let mut work = Work::new(Kind::Read);
    let reader = work.object.unwrap();
    let mut state = work.vm.heap.file_reader(reader).unwrap().clone();
    state.ready = 2;
    state.blob = None;
    work.vm
        .with_roots(|heap| heap.set_file_reader(reader, state))
        .unwrap();
    let roots = work.vm.heap.root_storage_state().0;
    work.vm.config.instruction_budget = 0;
    let _peer = leave_room(&mut work.vm, 0);
    work.vm.run_own_browser_task(0).unwrap();
    assert!(work.vm.browser_tasks.is_empty());
    assert_eq!(work.vm.heap.root_storage_state().0, roots - 1);
    assert_eq!(work.vm.remaining_instructions, 73);
    assert_eq!(work.vm.heap.file_reader(reader).unwrap().ready, 2);
}
#[cfg_attr(test, test)]
fn browser_task_entry_handled_rejection_cleanup_releases_roots_without_instruction() {
    let mut work = Work::new(Kind::Rejection);
    work.vm.mark_promise_handled(work.object.unwrap());
    let roots = work.vm.heap.root_storage_state().0;
    work.vm.config.instruction_budget = 0;
    let _peer = leave_room(&mut work.vm, 0);
    work.vm.run_own_browser_task(0).unwrap();
    assert!(work.vm.browser_tasks.is_empty());
    assert_eq!(work.vm.heap.root_storage_state().0, roots - 1);
    assert_eq!(work.vm.remaining_instructions, 73);
    assert_eq!(work.vm.host_rejections.as_ref().unwrap().active, 0);
}
#[cfg_attr(test, test)]
fn browser_task_entry_stale_completion_cleanup_keeps_the_settled_promise() {
    let (mut vm, token, promise) = completion_fixture();
    let payload = vm.charge_execution_payload(0);
    vm.run_host_promise_task(
        token,
        HostPromiseCompletion::Resolve(HostValue::Number(7.0)),
        payload,
    )
    .unwrap();
    let roots = vm.heap.root_storage_state();
    let fuel = vm.remaining_instructions;
    vm.config.instruction_budget = 0;
    vm.run_own_browser_task(0).unwrap();
    assert!(vm.browser_tasks.is_empty());
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.remaining_instructions, fuel);
    assert!(
        matches!(vm.promises[&promise].status,PromiseStatus::Fulfilled(Value::Number(value)) if value==7.0)
    );
}
#[cfg_attr(test, test)]
fn browser_task_entry_public_turn_zero_fuel_keeps_completion_delivery_queued() {
    let (mut vm, token, promise) = completion_fixture();
    vm.config.instruction_budget = 0;
    let roots = vm.heap.root_storage_state();
    assert!(matches!(
        vm.run_host_task_turn(0, 1),
        Err(RuntimeError::InstructionLimit)
    ));
    assert_eq!(vm.browser_tasks.len(), 1);
    assert!(vm.host_completion_queued_for_test(token));
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(matches!(
        vm.promises[&promise].status,
        PromiseStatus::Pending
    ));
}
#[cfg_attr(test, test)]
fn browser_task_entry_successful_attempts_keep_all_task_kinds_lifecycles() {
    for kind in KINDS {
        let mut work = Work::new(kind);
        work.vm.run_own_browser_task(0).unwrap();
        assert_eq!(work.vm.active_scheduling_bytes, 0);
        match kind {
            Kind::Read => {
                assert_eq!(work.vm.browser_tasks.len(), 1);
                assert_eq!(
                    work.vm
                        .heap
                        .file_reader(work.object.unwrap())
                        .unwrap()
                        .ready,
                    1
                );
            }
            Kind::Completion => {
                assert!(work.vm.browser_tasks.is_empty());
                assert!(work.vm.host_promises.pending.is_empty());
            }
            Kind::Timeout => {
                assert!(work.vm.browser_tasks.is_empty());
                assert!(work
                    .vm
                    .heap
                    .abort_signal(work.object.unwrap())
                    .unwrap()
                    .aborted());
            }
            Kind::Rejection => {
                assert!(work.vm.browser_tasks.is_empty());
                assert!(work.vm.promises[&work.object.unwrap()].rejection_notified);
                assert_eq!(work.vm.host_rejections.as_ref().unwrap().active, 0);
            }
            _ => assert!(work.vm.browser_tasks.is_empty()),
        }
    }
}
#[cfg_attr(test, test)]
fn browser_task_entry_preparation_refusal_keeps_the_callers_active_byte_context() {
    let (mut vm, _) = drag_fixture();
    vm.active_scheduling_bytes = 17;
    let _peer = leave_room(&mut vm, 0);
    let roots = vm.heap.root_storage_state();
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.active_scheduling_bytes, 17);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.remaining_instructions, 73);
}
#[cfg_attr(test, test)]
fn browser_task_entry_reporting_abort_releases_the_committed_drag_root() {
    let mut vm = page_vm();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    let scope = vm.host_scheduling.as_ref().unwrap().child();
    vm.install_host_function("callback", 1, move |_args: &[HostValue]| {
        scope.fail();
        Err(HostFunctionError::new("declined"))
    })
    .unwrap();
    let callback = vm
        .lookup_global_name("callback")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    vm.queue_drag_string_task(callback, "tiny".into()).unwrap();
    let roots = vm.heap.root_storage_state().0;
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.root_storage_state().0, roots - 1);
    assert!(vm.browser_tasks.is_empty());
    assert_eq!(vm.active_scheduling_bytes, 0);
}
#[cfg_attr(test, test)]
fn browser_task_entry_drag_callback_and_text_survive_gc_after_queue_removal() {
    let mut vm = page_vm();
    execute(&mut vm,"globalThis.seen=null;globalThis.marker=null;globalThis.cb=function(text){seen=text;marker={};};0;");
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    let callback = vm.heap.get(global, "cb").unwrap().object_id().unwrap();
    vm.queue_drag_string_task(callback, "tiny".into()).unwrap();
    vm.heap.set(global, "cb", Value::Undefined).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let mut garbage = Vec::new();
    for _ in 0..4 {
        garbage.push(vm.heap.alloc_object(None).unwrap());
    }
    vm.heap.allow_only(0);
    let collections = vm.heap.stats().major_collections;
    vm.run_own_browser_task(0).unwrap();
    assert!(vm.heap.stats().major_collections > collections);
    assert!(garbage.iter().all(|object| !vm.heap.contains(*object)));
    assert_eq!(
        vm.heap.get(global, "seen").unwrap(),
        Value::String("tiny".into())
    );
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(callback));
}
#[cfg_attr(test, test)]
fn browser_task_entry_vm_drop_refunds_every_kind_after_refused_preparation() {
    for kind in KINDS {
        let mut work = Work::new(kind);
        let observed = observer(&work.vm);
        work.vm.config.instruction_budget = 0;
        assert_eq!(
            work.vm.run_own_browser_task(0),
            Err(RuntimeError::InstructionLimit)
        );
        drop(work);
        assert_eq!(
            observed.host_scheduling_snapshot().payload_bytes,
            0,
            "{kind:?}"
        );
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_browser_task_entry_contracts() {
        browser_task_entry_zero_fuel_preserves_drag_task_root_and_context();
        browser_task_entry_argument_refusal_preserves_drag_task_root_and_context();
        browser_task_entry_zero_fuel_preserves_completion_task_wait_and_status();
        browser_task_entry_public_turn_zero_fuel_keeps_queued_drag_delivery();
        browser_task_entry_zero_fuel_preserves_every_active_task_kind();
        browser_task_entry_zero_fuel_precedes_native_admission_for_every_active_kind();
        browser_task_entry_non_drag_preparation_needs_no_argument_allocation();
        browser_task_entry_terminal_refusal_preserves_every_active_task_kind();
        browser_task_entry_drag_argument_exact_allowance_keeps_text_in_the_queue();
        browser_task_entry_rust_held_arguments_keep_their_lease_after_vm_drop();
        browser_task_entry_dropped_preparation_preserves_all_queue_owners();
        browser_task_entry_drag_zero_fuel_can_retry_the_same_delivery();
        browser_task_entry_completion_retry_spends_one_host_instruction_before_settlement();
        browser_task_entry_committed_drag_dispatch_failure_releases_its_root_and_payload();
        browser_task_entry_drag_text_and_argument_capacity_transfer_once_to_dispatch();
        browser_task_entry_drag_inputs_do_not_remain_charged_during_the_checkpoint();
        browser_task_entry_future_timeout_preserves_deadline_serial_and_ambient_fuel();
        browser_task_entry_dead_timeout_cleanup_needs_no_instruction_or_argument_storage();
        browser_task_entry_aborted_timeout_cleanup_releases_a_retained_task_root();
        browser_task_entry_stale_read_cleanup_releases_its_root_without_instruction();
        browser_task_entry_handled_rejection_cleanup_releases_roots_without_instruction();
        browser_task_entry_stale_completion_cleanup_keeps_the_settled_promise();
        browser_task_entry_public_turn_zero_fuel_keeps_completion_delivery_queued();
        browser_task_entry_successful_attempts_keep_all_task_kinds_lifecycles();
        browser_task_entry_preparation_refusal_keeps_the_callers_active_byte_context();
        browser_task_entry_reporting_abort_releases_the_committed_drag_root();
        browser_task_entry_drag_callback_and_text_survive_gc_after_queue_removal();
        browser_task_entry_vm_drop_refunds_every_kind_after_refused_preparation();
    }
}
