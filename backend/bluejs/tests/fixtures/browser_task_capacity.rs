// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny browser queues and scalar models of other document-owned storage.

use super::*;
use crate::vm::browser_tasks::BrowserTask;

fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}

fn page_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}

fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    // This is an abstract owner, not an allocation of the represented bytes.
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Producer {
    Drag,
    Timeout,
    Read,
    Completion,
    Unhandled,
    Handled,
}

struct Fixture {
    vm: Vm,
    callback: ObjectId,
    token: Option<HostPromiseToken>,
    promise: Option<ObjectId>,
    reader: Option<ObjectId>,
    blob: Option<ObjectId>,
}

impl Fixture {
    fn new(producer: Producer, entries: usize) -> Self {
        let mut vm = page_vm();
        let callback = execute(&mut vm, "globalThis.cb = function() {}; cb;")
            .object_id()
            .unwrap();
        let mut fixture = Self {
            vm,
            callback,
            token: None,
            promise: None,
            reader: None,
            blob: None,
        };
        match producer {
            Producer::Drag => {}
            Producer::Timeout | Producer::Read => {
                fixture.vm.install_web_file_api().unwrap();
                if matches!(producer, Producer::Read) {
                    fixture.reader = execute(
                        &mut fixture.vm,
                        "globalThis.reader = new FileReader(); reader;",
                    )
                    .object_id();
                    fixture.blob =
                        execute(&mut fixture.vm, "globalThis.blob = new Blob(['x']); blob;")
                            .object_id();
                }
            }
            Producer::Completion => {
                let token = Rc::new(RefCell::new(None));
                let output = token.clone();
                fixture
                    .vm
                    .install_host_async_function(
                        "request",
                        0,
                        HostTaskSource::Networking,
                        move |value, _args: &[HostValue]| {
                            *output.borrow_mut() = Some(value);
                            Ok(())
                        },
                    )
                    .unwrap();
                execute(&mut fixture.vm, "request(); 0;");
                fixture.token = *token.borrow();
            }
            Producer::Unhandled | Producer::Handled => {
                fixture.vm.install_web_file_api().unwrap();
                fixture.vm.install_host_console().unwrap();
                fixture.vm.install_host_error_reporting().unwrap();
                fixture.vm.install_host_rejection_tracking().unwrap();
                fixture.promise = execute(&mut fixture.vm, "onunhandledrejection = event => event.preventDefault(); globalThis.rejected = Promise.reject(0); rejected;").object_id();
                if matches!(producer, Producer::Handled) {
                    fixture.vm.notify_host_rejections().unwrap();
                    fixture.vm.run_own_browser_task(0).unwrap();
                    assert!(fixture.vm.browser_tasks.is_empty());
                }
            }
        }
        for _ in 0..entries {
            fixture
                .vm
                .queue_drag_string_task(callback, "".into())
                .unwrap();
        }
        fixture
    }

    fn append(&mut self, producer: Producer) -> Result<(), RuntimeError> {
        match producer {
            Producer::Drag => self.vm.queue_drag_string_task(self.callback, "".into()),
            Producer::Timeout => self
                .vm
                .abort_signal_timeout(&[Value::Number(1.0)])
                .map(|_| ()),
            Producer::Read => self
                .vm
                .web_reader_call(
                    crate::native::ReaderNative::Read(crate::heap::FileReadKind::Text),
                    &Value::Object(self.reader.unwrap()),
                    &[Value::Object(self.blob.unwrap())],
                    false,
                )
                .map(|_| ()),
            Producer::Completion => self
                .vm
                .queue_host_promise_completion(
                    self.token.unwrap(),
                    HostPromiseCompletion::Resolve(HostValue::Number(0.0)),
                )
                .map(|accepted| assert!(accepted)),
            Producer::Unhandled => self.vm.notify_host_rejections(),
            Producer::Handled => {
                self.vm.mark_promise_handled(self.promise.unwrap());
                self.vm.check_scheduling_failure()
            }
        }
    }

    fn extra_payload(producer: Producer) -> usize {
        if matches!(producer, Producer::Handled) {
            std::mem::size_of::<host_rejections::PendingRejection>()
        } else {
            0
        }
    }
}

fn assert_refused(producer: Producer) {
    let mut fixture = Fixture::new(producer, 8);
    let capacity = fixture.vm.browser_tasks.capacity();
    assert_eq!(capacity, 8);
    let pending = fixture
        .vm
        .host_rejections
        .as_ref()
        .map(|state| state.pending.len());
    let roots = fixture.vm.heap.stats().root_registrations;
    let serial = fixture
        .vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .account
        .borrow()
        .next_task_serial;
    let _owner = leave_room(
        &mut fixture.vm,
        capacity * std::mem::size_of::<BrowserTask>() + Fixture::extra_payload(producer) - 1,
    );
    assert_eq!(
        fixture.append(producer),
        Err(RuntimeError::SchedulingLimit),
        "{producer:?}"
    );
    assert_eq!(fixture.vm.browser_tasks.len(), 8, "{producer:?}");
    assert_eq!(fixture.vm.browser_tasks.capacity(), capacity);
    assert_eq!(fixture.vm.heap.stats().root_registrations, roots);
    assert_eq!(
        fixture
            .vm
            .host_scheduling
            .as_ref()
            .unwrap()
            .account
            .borrow()
            .next_task_serial,
        serial
    );
    assert_eq!(
        fixture
            .vm
            .host_rejections
            .as_ref()
            .map(|state| state.pending.len()),
        pending
    );
    if let Some(reader) = fixture.reader {
        let state = fixture.vm.heap.file_reader(reader).unwrap();
        assert_eq!(state.ready, 0);
        assert_eq!(state.epoch, 0);
    }
}

fn assert_existing_slot(producer: Producer) {
    let mut fixture = Fixture::new(producer, 5);
    let capacity = fixture.vm.browser_tasks.capacity();
    assert!(capacity > fixture.vm.browser_tasks.len());
    let _owner = leave_room(&mut fixture.vm, Fixture::extra_payload(producer));
    assert_eq!(fixture.append(producer), Ok(()), "{producer:?}");
    assert_eq!(fixture.vm.browser_tasks.len(), 6);
    assert_eq!(fixture.vm.browser_tasks.capacity(), capacity);
    assert_eq!(fixture.vm.check_scheduling_failure(), Ok(()));
    if let Some(reader) = fixture.reader {
        assert_eq!(fixture.vm.heap.file_reader(reader).unwrap().ready, 1);
    }
    if matches!(producer, Producer::Unhandled) {
        assert!(fixture
            .vm
            .host_rejections
            .as_ref()
            .unwrap()
            .pending
            .is_empty());
    }
}

macro_rules! producer_tests {
    ($(($refused:ident, $fits:ident, $producer:ident)),* $(,)?) => {
        $(
            #[cfg_attr(test, test)]
            fn $refused() { assert_refused(Producer::$producer); }
            #[cfg_attr(test, test)]
            fn $fits() { assert_existing_slot(Producer::$producer); }
        )*
        fn verify_producer_contracts() {
            $( $refused(); $fits(); )*
        }
    };
}

producer_tests!(
    (
        drag_growth_is_refused_before_publication,
        drag_uses_existing_slots,
        Drag
    ),
    (
        timeout_growth_is_refused_before_publication,
        timeout_uses_existing_slots,
        Timeout
    ),
    (
        read_growth_is_refused_before_state_changes,
        read_uses_existing_slots,
        Read
    ),
    (
        completion_growth_is_refused_before_publication,
        completion_uses_existing_slots,
        Completion
    ),
    (
        unhandled_growth_keeps_pending_notifications,
        unhandled_uses_existing_slots,
        Unhandled
    ),
    (
        handled_growth_is_refused_before_publication,
        handled_uses_existing_slots,
        Handled
    ),
);

fn cleanup_vm(entries: usize) -> Vm {
    let mut vm = page_vm();
    execute(&mut vm, &format!("var registries=[]; for(var i=0;i<{entries};i++) {{ var registry=new FinalizationRegistry(()=>{{}}); registries.push(registry); registry.register({{}},0); }} let target={{}}; var last=new FinalizationRegistry(()=>{{}}); last.register(target,0); 0;"));
    vm.collect_realm_garbage().unwrap();
    vm.enqueue_host_finalization_tasks().unwrap();
    assert_eq!(vm.host_finalization_tasks.len(), entries);
    // A full script exit would enqueue cleanup before the admission under test.
    assert!(vm.set_global_binding("target", Value::Null).unwrap());
    vm.collect_realm_garbage().unwrap();
    vm
}

#[cfg_attr(test, test)]
fn cleanup_growth_is_refused_before_roots_or_tasks_are_retained() {
    let mut vm = cleanup_vm(8);
    let capacity = vm.host_finalization_tasks.capacity();
    assert_eq!(capacity, 8);
    let roots = vm.heap.stats().root_registrations;
    let _owner = leave_room(
        &mut vm,
        capacity * std::mem::size_of::<host_finalization::HostFinalizationTask>() - 1,
    );
    assert_eq!(
        vm.enqueue_host_finalization_tasks(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_finalization_tasks.len(), 8);
    assert_eq!(vm.host_finalization_tasks.capacity(), capacity);
    assert_eq!(vm.heap.stats().root_registrations, roots);
}

#[cfg_attr(test, test)]
fn cleanup_uses_existing_slots() {
    let mut vm = cleanup_vm(5);
    let capacity = vm.host_finalization_tasks.capacity();
    assert!(capacity > vm.host_finalization_tasks.len());
    let _owner = leave_room(&mut vm, 0);
    assert_eq!(vm.enqueue_host_finalization_tasks(), Ok(()));
    assert_eq!(vm.host_finalization_tasks.len(), 6);
    assert_eq!(vm.host_finalization_tasks.capacity(), capacity);
}

fn pending_rejection_vm(entries: usize) -> (Vm, ObjectId) {
    let mut fixture = Fixture::new(Producer::Unhandled, 0);
    execute(&mut fixture.vm, &format!("var rejectedList=[rejected]; for(var i=1;i<{entries};i++) rejectedList.push(Promise.reject(0)); var nextPromise=new Promise(()=>{{}}); 0;"));
    let next = fixture
        .vm
        .lookup_global_name("nextPromise")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    assert_eq!(
        fixture.vm.host_rejections.as_ref().unwrap().pending.len(),
        entries
    );
    fixture.vm.promises.get_mut(&next).unwrap().status =
        PromiseStatus::Rejected(Value::Number(0.0));
    (fixture.vm, next)
}

#[cfg_attr(test, test)]
fn rejection_growth_is_refused_before_roots_or_pending_entries_change() {
    let (mut vm, promise) = pending_rejection_vm(8);
    let capacity = vm.host_rejections.as_ref().unwrap().pending.capacity();
    assert_eq!(capacity, 8);
    let roots = vm.heap.stats().root_registrations;
    let _owner = leave_room(
        &mut vm,
        capacity * std::mem::size_of::<host_rejections::PendingRejection>() - 1,
    );
    vm.track_host_rejection(promise);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.len(), 8);
    assert_eq!(
        vm.host_rejections.as_ref().unwrap().pending.capacity(),
        capacity
    );
    assert_eq!(vm.heap.stats().root_registrations, roots);
}

#[cfg_attr(test, test)]
fn rejection_uses_existing_slots() {
    let (mut vm, promise) = pending_rejection_vm(5);
    let capacity = vm.host_rejections.as_ref().unwrap().pending.capacity();
    assert!(capacity > 5);
    let _owner = leave_room(&mut vm, 0);
    vm.track_host_rejection(promise);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.len(), 6);
    assert_eq!(
        vm.host_rejections.as_ref().unwrap().pending.capacity(),
        capacity
    );
}

fn assert_file_requeue_refused(count_limit: bool) {
    let mut vm = page_vm();
    vm.install_web_file_api().unwrap();
    // Native capacity growth must exceed the callback's own frame cost so
    // refusal occurs at successor admission after the finite callback returns.
    let entries = if count_limit { 8 } else { 32 };
    execute(&mut vm, &format!("var reader=new FileReader(); reader.onloadstart=()=>{{for(var i=0;i<{entries};i++) AbortSignal.timeout(1);}}; reader.readAsText(new Blob(['x'])); 0;"));
    assert_eq!(vm.browser_tasks.capacity(), 4);
    let root = match &vm.browser_tasks[0] {
        BrowserTask::FileRead { root, .. } => *root,
        _ => unreachable!(),
    };
    let mut peer = vm.host_scheduling.as_ref().unwrap().child();
    let _owner = if count_limit {
        peer.check(
            HostSchedulingUsage {
                pending_browser_tasks: MAX_PENDING_BROWSER_TASKS - 8,
                ..Default::default()
            },
            HostSchedulingUsage::default(),
        )
        .unwrap();
        None
    } else {
        Some(leave_room(
            &mut vm,
            (2 * entries - 4) * std::mem::size_of::<BrowserTask>() - 1,
        ))
    };
    assert_eq!(
        vm.run_own_browser_task(0),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.browser_tasks.len(), entries, "count_limit={count_limit}");
    assert_eq!(vm.browser_tasks.capacity(), entries);
    assert!(
        vm.heap.unroot(root).is_err(),
        "refused requeue must release its reader root"
    );
}

#[cfg_attr(test, test)]
fn file_requeue_refuses_callback_consumed_capacity() {
    assert_file_requeue_refused(false);
}

#[cfg_attr(test, test)]
fn file_requeue_refuses_callback_consumed_document_count() {
    assert_file_requeue_refused(true);
}

#[cfg_attr(test, test)]
fn file_cancellation_preserves_existing_non_file_slots() {
    let mut fixture = Fixture::new(Producer::Read, 5);
    fixture.vm.browser_tasks.shrink_to_fit();
    let capacity = fixture.vm.browser_tasks.capacity();
    assert_eq!(capacity, 5);
    let roots = fixture.vm.heap.stats().root_registrations;
    let _owner = leave_room(&mut fixture.vm, 0);
    assert_eq!(
        fixture.vm.cancel_file_read_tasks(fixture.reader.unwrap()),
        Ok(())
    );
    assert_eq!(fixture.vm.browser_tasks.len(), 5);
    assert_eq!(fixture.vm.browser_tasks.capacity(), capacity);
    assert_eq!(fixture.vm.heap.stats().root_registrations, roots);
}

#[cfg_attr(test, test)]
fn timeout_clock_enqueues_precede_the_outer_timeout_serial() {
    let mut vm = page_vm();
    vm.install_web_file_api().unwrap();
    let mut child = page_vm();
    child.host_scheduling = Some(vm.host_scheduling.as_ref().unwrap().child());
    child.install_web_file_api().unwrap();
    let child = Rc::new(RefCell::new(child));
    let queued = child.clone();
    let sampled = std::cell::Cell::new(false);
    vm.install_host_timer_functions_with_clock(move || {
        if !sampled.replace(true) {
            queued
                .borrow_mut()
                .abort_signal_timeout(&[Value::Number(0.0)])
                .unwrap();
        }
        0
    })
    .unwrap();
    vm.abort_signal_timeout(&[Value::Number(0.0)]).unwrap();
    assert!(child.borrow().browser_tasks[0].order() < vm.browser_tasks[0].order());
}

impl crate::Vm {
    /// Runs browser scheduling capacity contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_browser_task_capacity_contracts() {
        verify_producer_contracts();
        cleanup_growth_is_refused_before_roots_or_tasks_are_retained();
        cleanup_uses_existing_slots();
        rejection_growth_is_refused_before_roots_or_pending_entries_change();
        rejection_uses_existing_slots();
        file_requeue_refuses_callback_consumed_capacity();
        file_requeue_refuses_callback_consumed_document_count();
        file_cancellation_preserves_existing_non_file_slots();
        timeout_clock_enqueues_precede_the_outer_timeout_serial();
    }
}
