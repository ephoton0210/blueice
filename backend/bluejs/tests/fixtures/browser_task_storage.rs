// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite in-memory tasks and scalar allowances verify producer transactions.

use super::*;
use crate::vm::browser_tasks::BrowserTask;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}
fn serial(vm: &Vm) -> u64 {
    vm.host_scheduling.as_ref().unwrap().task_serial_state()
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn fill_roots(vm: &mut Vm, free: usize) {
    let (mut len, mut capacity, _) = vm.heap.root_storage_state();
    assert!(capacity <= 1023);
    if capacity - len < free {
        for _ in 0..=capacity - len {
            vm.heap.root(vm.object_prototype).unwrap();
        }
        (len, capacity, _) = vm.heap.root_storage_state();
    }
    for _ in len..capacity - free {
        vm.heap.root(vm.object_prototype).unwrap();
    }
    vm.synchronize_scheduling_usage().unwrap();
}
fn drag_fixture(count: usize) -> (Vm, ObjectId) {
    let mut vm = page_vm();
    vm.install_host_function("callback", 1, |_args: &[HostValue]| {
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm
        .lookup_global_name("callback")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    for _ in 0..count {
        vm.queue_drag_string_task(callback, "".into()).unwrap();
    }
    (vm, callback)
}
fn reader_fixture() -> (Vm, ObjectId, ObjectId) {
    let mut vm = page_vm();
    vm.install_web_file_api().unwrap();
    execute(
        &mut vm,
        "var reader=new FileReader(); var blob=new Blob(['x']); 0;",
    );
    let reader = vm
        .lookup_global_name("reader")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    let blob = vm
        .lookup_global_name("blob")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    (vm, reader, blob)
}
fn start_read(vm: &mut Vm, reader: ObjectId, blob: ObjectId) -> Result<Value, RuntimeError> {
    vm.web_reader_call(
        crate::native::ReaderNative::Read(crate::heap::FileReadKind::Text),
        &Value::Object(reader),
        &[Value::Object(blob)],
        false,
    )
}

#[cfg_attr(test, test)]
fn browser_task_storage_drag_refuses_net_only_replacement_allowance() {
    let (mut vm, callback) = drag_fixture(4);
    fill_roots(&mut vm, 1);
    let roots = vm.heap.root_storage_state();
    let next = serial(&vm);
    let _peer = leave_room(
        &mut vm,
        4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>(),
    );
    assert_eq!(
        vm.queue_drag_string_task(callback, "".into()),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(
        (vm.browser_tasks.len(), vm.browser_tasks.capacity()),
        (4, 4)
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(serial(&vm), next);
}
#[cfg_attr(test, test)]
fn browser_task_storage_reader_root_refusal_preserves_serial_and_queue_capacity() {
    let (mut vm, reader, blob) = reader_fixture();
    vm.heap.allow_root_registrations(0);
    let roots = vm.heap.root_storage_state();
    let next = serial(&vm);
    assert_eq!(
        start_read(&mut vm, reader, blob),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(serial(&vm), next);
    assert_eq!(
        (vm.browser_tasks.len(), vm.browser_tasks.capacity()),
        (0, 0)
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.heap.file_reader(reader).unwrap().ready, 0);
}
#[cfg_attr(test, test)]
fn browser_task_storage_timeout_managed_refusal_preserves_serial_and_queue_capacity() {
    let mut vm = page_vm();
    vm.install_web_file_api().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm.heap.allow_only(0);
    let next = serial(&vm);
    assert!(matches!(
        vm.abort_signal_timeout(&[Value::Number(1.0)]),
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    assert_eq!(serial(&vm), next);
    assert_eq!(
        (vm.browser_tasks.len(), vm.browser_tasks.capacity()),
        (0, 0)
    );
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
const PRODUCERS: [Producer; 6] = [
    Producer::Drag,
    Producer::Timeout,
    Producer::Read,
    Producer::Completion,
    Producer::Unhandled,
    Producer::Handled,
];
struct Fixture {
    vm: Vm,
    producer: Producer,
    callback: ObjectId,
    reader: Option<(ObjectId, ObjectId)>,
    token: Option<HostPromiseToken>,
    promise: Option<ObjectId>,
}
impl Fixture {
    fn new(producer: Producer, count: usize) -> Self {
        let (mut vm, reader) = if matches!(producer, Producer::Read) {
            let (vm, reader, blob) = reader_fixture();
            (vm, Some((reader, blob)))
        } else {
            (page_vm(), None)
        };
        vm.install_host_function("callback", 1, |_args: &[HostValue]| {
            Ok(HostValue::Undefined)
        })
        .unwrap();
        let callback = vm
            .lookup_global_name("callback")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        let mut token = None;
        let mut promise = None;
        match producer {
            Producer::Timeout => {
                vm.install_web_file_api().unwrap();
            }
            Producer::Completion => {
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
                vm.host_async_function_call(0, Value::Undefined, &[], false)
                    .unwrap();
                token = *saved.borrow();
            }
            Producer::Unhandled | Producer::Handled => {
                vm.host_rejections = Some(Default::default());
                let object = vm.new_promise().unwrap();
                vm.heap.root(object).unwrap();
                vm.promises.get_mut(&object).unwrap().status =
                    PromiseStatus::Rejected(Value::Number(1.0));
                if matches!(producer, Producer::Unhandled) {
                    vm.track_host_rejection(object);
                } else {
                    vm.promises.get_mut(&object).unwrap().rejection_notified = true;
                }
                promise = Some(object);
            }
            _ => {}
        }
        for _ in 0..count {
            vm.queue_drag_string_task(callback, "".into()).unwrap();
        }
        vm.with_roots(|heap| heap.try_collect_major()).unwrap();
        Self {
            vm,
            producer,
            callback,
            reader,
            token,
            promise,
        }
    }
    fn append(&mut self) -> Result<(), RuntimeError> {
        match self.producer {
            Producer::Drag => self.vm.queue_drag_string_task(self.callback, "".into()),
            Producer::Timeout => self
                .vm
                .abort_signal_timeout(&[Value::Number(1.0)])
                .map(|_| ()),
            Producer::Read => {
                let (reader, blob) = self.reader.unwrap();
                start_read(&mut self.vm, reader, blob).map(|_| ())
            }
            Producer::Completion => self
                .vm
                .queue_host_promise_completion(
                    self.token.unwrap(),
                    HostPromiseCompletion::Resolve(HostValue::Number(1.0)),
                )
                .map(|accepted| assert!(accepted)),
            Producer::Unhandled => self.vm.notify_host_rejections(),
            Producer::Handled => {
                self.vm.mark_promise_handled(self.promise.unwrap());
                self.vm.check_scheduling_failure()
            }
        }
    }
    fn extra_bytes(&self) -> usize {
        match self.producer {
            // The independent reader copy plus the call's temporary realm roots.
            Producer::Read => {
                std::mem::size_of::<crate::heap::FileReaderState>() + realm_roots_bytes(4)
            }
            Producer::Timeout => realm_roots_bytes(0),
            Producer::Handled => std::mem::size_of::<host_rejections::PendingRejection>(),
            _ => 0,
        }
    }
    fn assert_unpublished(&self) {
        if let Some((reader, _)) = self.reader {
            let state = self.vm.heap.file_reader(reader).unwrap();
            assert_eq!(
                (state.ready, state.epoch, state.loaded, state.total),
                (0, 0, 0, 0)
            );
            assert_eq!(state.result, Value::Null);
            assert!(state.blob.is_none() && state.encoding.is_none());
        }
        if let Some(token) = self.token {
            assert!(!self.vm.host_completion_queued_for_test(token));
        }
        if let Some(promise) = self.promise {
            if matches!(self.producer, Producer::Unhandled) {
                assert_eq!(self.vm.host_rejections.as_ref().unwrap().pending.len(), 1);
            } else {
                assert!(self.vm.promises[&promise].handled);
                assert!(self.vm.promises[&promise].rejection_notified);
            }
        }
    }
}
fn root_bytes(vm: &Vm) -> usize {
    let (len, cap, _) = vm.heap.root_storage_state();
    assert_eq!(len, cap);
    host_scheduling::map_slots(&HashMap::<RootId, ObjectId>::with_capacity(
        (len + 1).max(len * 2),
    ))
}
fn realm_roots_bytes(count: usize) -> usize {
    crate::vm::realm_call_storage::RealmCallRoots::new(count, None)
        .unwrap()
        .retained_bytes()
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}

#[cfg_attr(test, test)]
fn browser_task_storage_all_producers_refuse_net_only_growth() {
    for producer in PRODUCERS {
        let mut fixture = Fixture::new(producer, 4);
        fill_roots(&mut fixture.vm, 1);
        let room = 4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>()
            + fixture.extra_bytes();
        let roots = fixture.vm.heap.root_storage_state();
        let next = serial(&fixture.vm);
        let _peer = leave_room(&mut fixture.vm, room);
        assert_eq!(
            fixture.append(),
            Err(RuntimeError::SchedulingLimit),
            "{producer:?}"
        );
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (4, 4)
        );
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(serial(&fixture.vm), next);
        fixture.assert_unpublished();
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_full_growth_allowance_refunds_old_backing_and_reader_copy() {
    for producer in PRODUCERS {
        let mut fixture = Fixture::new(producer, 4);
        fill_roots(&mut fixture.vm, 1);
        let extra = fixture.extra_bytes();
        let old = 4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>();
        let _peer = leave_room(&mut fixture.vm, 2 * old + extra);
        fixture.append().unwrap();
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (5, 8)
        );
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES
                - old
                - if matches!(producer, Producer::Read | Producer::Timeout) {
                    // The reader copy and temporary realm roots are all refunded.
                    extra
                } else {
                    0
                },
            "{producer:?}"
        );
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_one_byte_short_growth_refunds_all_preparations() {
    for producer in PRODUCERS {
        let mut fixture = Fixture::new(producer, 4);
        fill_roots(&mut fixture.vm, 1);
        let room = 8 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>()
            + fixture.extra_bytes()
            - 1;
        let roots = fixture.vm.heap.root_storage_state();
        let next = serial(&fixture.vm);
        let _peer = leave_room(&mut fixture.vm, room);
        assert_eq!(
            fixture.append(),
            Err(RuntimeError::SchedulingLimit),
            "{producer:?}"
        );
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (4, 4)
        );
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(serial(&fixture.vm), next);
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - room
        );
        fixture.assert_unpublished();
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_spare_slots_admit_only_new_owners() {
    for producer in PRODUCERS {
        let mut fixture = Fixture::new(producer, 2);
        fill_roots(&mut fixture.vm, 1);
        let extra = fixture.extra_bytes();
        let _peer = leave_room(&mut fixture.vm, extra);
        fixture.append().unwrap();
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (3, 4)
        );
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES
                - if matches!(producer, Producer::Read | Producer::Timeout) {
                    // The reader copy and temporary realm roots are all refunded.
                    extra
                } else {
                    0
                },
            "{producer:?}"
        );
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_serial_refusal_preserves_every_producer_state() {
    for producer in PRODUCERS {
        let mut fixture = Fixture::new(producer, 0);
        fixture
            .vm
            .host_scheduling
            .as_ref()
            .unwrap()
            .set_task_serial_for_test(u64::MAX);
        let roots = fixture.vm.heap.root_storage_state();
        assert_eq!(
            fixture.append(),
            Err(RuntimeError::SchedulingLimit),
            "{producer:?}"
        );
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (0, 0)
        );
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(serial(&fixture.vm), u64::MAX);
        fixture.assert_unpublished();
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_shared_count_refusal_preserves_every_producer_state() {
    for producer in PRODUCERS {
        let mut fixture = Fixture::new(producer, 0);
        let mut peer = fixture.vm.host_scheduling.as_ref().unwrap().child();
        peer.check(
            HostSchedulingUsage {
                pending_browser_tasks: MAX_PENDING_BROWSER_TASKS,
                ..Default::default()
            },
            HostSchedulingUsage::default(),
        )
        .unwrap();
        let roots = fixture.vm.heap.root_storage_state();
        let next = serial(&fixture.vm);
        assert_eq!(fixture.append(), Err(RuntimeError::SchedulingLimit));
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (0, 0)
        );
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(serial(&fixture.vm), next);
        fixture.assert_unpublished();
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_root_id_refusal_preserves_rooted_producers() {
    for producer in [Producer::Drag, Producer::Read, Producer::Handled] {
        let mut fixture = Fixture::new(producer, 0);
        fixture.vm.heap.allow_root_registrations(0);
        let roots = fixture.vm.heap.root_storage_state();
        let next = serial(&fixture.vm);
        let expected = if matches!(producer, Producer::Handled) {
            RuntimeError::SchedulingLimit
        } else {
            RuntimeError::Heap(HeapError::IdExhausted)
        };
        assert_eq!(fixture.append(), Err(expected));
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (0, 0)
        );
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(serial(&fixture.vm), next);
        fixture.assert_unpublished();
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_joint_root_refusal_preserves_queue_and_reader_state() {
    for producer in [Producer::Drag, Producer::Read, Producer::Handled] {
        let mut fixture = Fixture::new(producer, 0);
        fill_roots(&mut fixture.vm, 0);
        let room = 4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>()
            + fixture.extra_bytes()
            + root_bytes(&fixture.vm)
            - 1;
        let roots = fixture.vm.heap.root_storage_state();
        let next = serial(&fixture.vm);
        let _peer = leave_room(&mut fixture.vm, room);
        assert_eq!(
            fixture.append(),
            Err(RuntimeError::SchedulingLimit),
            "{producer:?}"
        );
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (0, 0)
        );
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(serial(&fixture.vm), next);
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - room
        );
        fixture.assert_unpublished();
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_exact_joint_root_allowance_transfers_each_owner_once() {
    for producer in [Producer::Drag, Producer::Read, Producer::Handled] {
        let mut fixture = Fixture::new(producer, 0);
        fill_roots(&mut fixture.vm, 0);
        let extra = fixture.extra_bytes();
        let old = fixture.vm.heap.root_storage_bytes();
        let room = 4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>()
            + extra
            + root_bytes(&fixture.vm);
        let _peer = leave_room(&mut fixture.vm, room);
        fixture.append().unwrap();
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (1, 4)
        );
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES
                - old
                - if matches!(producer, Producer::Read | Producer::Timeout) {
                    // The reader copy and temporary realm roots are all refunded.
                    extra
                } else {
                    0
                },
            "{producer:?}"
        );
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_vm_drop_refunds_each_committed_producer() {
    for producer in PRODUCERS {
        let mut fixture = Fixture::new(producer, 0);
        fixture.append().unwrap();
        let observed = observer(&fixture.vm);
        drop(fixture);
        assert_eq!(
            observed.host_scheduling_snapshot().payload_bytes,
            0,
            "{producer:?}"
        );
    }
}
#[cfg_attr(test, test)]
fn browser_task_storage_rust_held_candidate_keeps_its_document_lease_after_vm_drop() {
    let (mut vm, _) = drag_fixture(0);
    let text: JsString = "tiny".into();
    let prepared = vm
        .prepare_browser_task(HostSchedulingUsage {
            payload_bytes: 8,
            ..Default::default()
        })
        .unwrap();
    let observed = observer(&vm);
    drop(vm);
    assert_eq!(
        observed.host_scheduling_snapshot().payload_bytes,
        4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>() + 8
    );
    drop(text);
    drop(prepared);
    assert_eq!(observed.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn browser_task_storage_drag_payload_exact_allowance_transfers_owned_text() {
    let (mut vm, callback) = drag_fixture(0);
    fill_roots(&mut vm, 1);
    let _peer = leave_room(
        &mut vm,
        4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>() + 8,
    );
    vm.queue_drag_string_task(callback, "tiny".into()).unwrap();
    let BrowserTask::DragString { text, .. } = &vm.browser_tasks[0] else {
        panic!("drag task")
    };
    assert_eq!(text, &JsString::from("tiny"));
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn browser_task_storage_completion_payload_refusal_keeps_the_wait_unqueued() {
    let mut fixture = Fixture::new(Producer::Completion, 0);
    let room = 4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>() + 7;
    let next = serial(&fixture.vm);
    let _peer = leave_room(&mut fixture.vm, room);
    assert_eq!(
        fixture.vm.queue_host_promise_completion(
            fixture.token.unwrap(),
            HostPromiseCompletion::Resolve(HostValue::String("tiny".into()))
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(fixture.vm.browser_tasks.capacity(), 0);
    assert_eq!(serial(&fixture.vm), next);
    fixture.assert_unpublished();
}
#[cfg_attr(test, test)]
fn browser_task_storage_reader_managed_refusal_preserves_initial_state() {
    let (mut vm, reader, blob) = reader_fixture();
    vm.heap.allow_only(0);
    let roots = vm.heap.root_storage_state();
    let next = serial(&vm);
    let result = vm.web_reader_call(
        crate::native::ReaderNative::Read(crate::heap::FileReadKind::Text),
        &Value::Object(reader),
        &[Value::Object(blob), Value::String("utf-8".into())],
        false,
    );
    assert!(matches!(
        result,
        Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
    ));
    let state = vm.heap.file_reader(reader).unwrap();
    assert_eq!((state.ready, state.epoch), (0, 0));
    assert!(state.encoding.is_none() && state.blob.is_none());
    assert_eq!(vm.browser_tasks.capacity(), 0);
    assert_eq!(serial(&vm), next);
    assert_eq!(vm.heap.root_storage_state(), roots);
}
#[cfg_attr(test, test)]
fn browser_task_storage_reader_listener_copy_exact_allowance_refunds_nested_owners() {
    let (mut vm, reader, blob) = reader_fixture();
    execute(&mut vm, "reader.onload=function(){};0;");
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    fill_roots(&mut vm, 1);
    let state = vm.heap.file_reader(reader).unwrap();
    assert_eq!(state.target.listeners.len(), 1);
    assert_eq!(state.target.listeners[0].name, JsString::from("load"));
    let copied = std::mem::size_of::<crate::heap::FileReaderState>()
        + std::mem::size_of::<crate::heap::WebListener>()
        + 8;
    let _peer = leave_room(
        &mut vm,
        4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>()
            + copied
            + realm_roots_bytes(4),
    );
    start_read(&mut vm, reader, blob).unwrap();
    assert_eq!(
        vm.heap.file_reader(reader).unwrap().target.listeners.len(),
        1
    );
    assert_eq!(vm.heap.file_reader(reader).unwrap().ready, 1);
    // The copy and the call's temporary roots are all refunded.
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - copied - realm_roots_bytes(4)
    );
}
#[cfg_attr(test, test)]
fn browser_task_storage_reader_listener_copy_refusal_preserves_original_backing() {
    let (mut vm, reader, blob) = reader_fixture();
    execute(&mut vm, "reader.onload=function(){};0;");
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    fill_roots(&mut vm, 1);
    let original = vm
        .heap
        .file_reader(reader)
        .unwrap()
        .target
        .listeners
        .capacity();
    let copied = std::mem::size_of::<crate::heap::FileReaderState>()
        + std::mem::size_of::<crate::heap::WebListener>()
        + 8;
    let room =
        4 * std::mem::size_of::<crate::vm::browser_task_storage::QueuedBrowserTask>() + copied - 1;
    let roots = vm.heap.root_storage_state();
    let next = serial(&vm);
    let _peer = leave_room(&mut vm, room);
    assert_eq!(
        start_read(&mut vm, reader, blob),
        Err(RuntimeError::SchedulingLimit)
    );
    let state = vm.heap.file_reader(reader).unwrap();
    assert_eq!(
        (state.ready, state.epoch, state.target.listeners.capacity()),
        (0, 0, original)
    );
    assert_eq!(vm.browser_tasks.capacity(), 0);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(serial(&vm), next);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - room
    );
}
#[cfg_attr(test, test)]
fn browser_task_storage_reader_epoch_refusal_precedes_task_admission() {
    let (mut vm, reader, blob) = reader_fixture();
    let mut state = vm.heap.file_reader(reader).unwrap().clone();
    state.epoch = u64::MAX;
    vm.with_roots(|heap| heap.set_file_reader(reader, state))
        .unwrap();
    let next = serial(&vm);
    let roots = vm.heap.root_storage_state();
    // Room for the call's temporary roots only: the epoch is refused before
    // any task slot or reader copy is admitted.
    let _peer = leave_room(&mut vm, realm_roots_bytes(4));
    assert!(matches!(
        start_read(&mut vm, reader, blob),
        Err(RuntimeError::RangeError(_))
    ));
    assert_eq!(vm.heap.file_reader(reader).unwrap().epoch, u64::MAX);
    assert_eq!(vm.heap.file_reader(reader).unwrap().ready, 0);
    assert_eq!(vm.browser_tasks.capacity(), 0);
    assert_eq!(serial(&vm), next);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn browser_task_storage_dropped_prepared_reader_keeps_the_original_state() {
    let (mut vm, reader, blob) = reader_fixture();
    let mut state = vm.heap.file_reader(reader).unwrap().clone();
    state.ready = 1;
    state.blob = Some(blob);
    state.epoch = 1;
    let prepared = vm
        .with_roots(|heap| heap.prepare_file_reader_update(reader, state))
        .unwrap();
    assert_eq!(vm.heap.file_reader(reader).unwrap().ready, 0);
    drop(prepared);
    assert_eq!(vm.heap.file_reader(reader).unwrap().epoch, 0);
    assert!(vm.heap.file_reader(reader).unwrap().blob.is_none());
}
#[cfg_attr(test, test)]
fn browser_task_storage_committed_reader_update_retains_a_young_blob_through_minor_gc() {
    let (mut vm, reader, original) = reader_fixture();
    let data = vm.heap.web_blob(original).unwrap().clone();
    let prototype = vm.globals["%WebBlob%"];
    let blob = vm
        .with_roots(|heap| heap.alloc_web_blob(data, prototype))
        .unwrap();
    let mut state = vm.heap.file_reader(reader).unwrap().clone();
    state.ready = 1;
    state.blob = Some(blob);
    state.epoch = 1;
    let prepared = vm
        .with_roots(|heap| heap.prepare_file_reader_update(reader, state))
        .unwrap();
    vm.heap.commit_file_reader_update(prepared);
    vm.with_roots(|heap| heap.try_collect_minor()).unwrap();
    assert_eq!(vm.heap.file_reader(reader).unwrap().blob, Some(blob));
    assert!(vm.heap.contains(blob));
}
#[cfg_attr(test, test)]
fn browser_task_storage_reader_preparation_protects_receiver_and_incoming_edges_during_gc() {
    let mut heap = Heap::default();
    let prototype = heap.alloc_object(None).unwrap();
    heap.root(prototype).unwrap();
    let reader = heap.alloc_file_reader(prototype).unwrap();
    let blob = heap
        .alloc_web_blob(
            crate::heap::WebBlobData {
                bytes: vec![1],
                media_type: "".into(),
                file: None,
            },
            prototype,
        )
        .unwrap();
    let garbage = heap.alloc_object(None).unwrap();
    let mut state = heap.file_reader(reader).unwrap().clone();
    state.blob = Some(blob);
    state.encoding = Some("tiny".into());
    heap.allow_only(0);
    let collections = heap.stats().major_collections;
    let prepared = heap.prepare_file_reader_update(reader, state).unwrap();
    assert!(heap.stats().major_collections > collections);
    assert!(heap.contains(reader) && heap.contains(blob));
    assert!(!heap.contains(garbage));
    assert!(heap.file_reader(reader).unwrap().blob.is_none());
    drop(prepared);
    heap.try_collect_major().unwrap();
    assert!(!heap.contains(reader) && !heap.contains(blob));
}
#[cfg_attr(test, test)]
fn browser_task_storage_reader_start_and_abort_release_the_committed_task() {
    let (mut vm, reader, blob) = reader_fixture();
    start_read(&mut vm, reader, blob).unwrap();
    assert_eq!(vm.heap.file_reader(reader).unwrap().ready, 1);
    assert_eq!(vm.browser_tasks.len(), 1);
    vm.web_reader_call(
        crate::native::ReaderNative::Abort,
        &Value::Object(reader),
        &[],
        false,
    )
    .unwrap();
    assert!(vm.browser_tasks.is_empty());
    assert_eq!(vm.heap.file_reader(reader).unwrap().ready, 2);
}
#[cfg_attr(test, test)]
fn browser_task_storage_timeout_serial_refusal_leaves_unpublished_signal_collectible() {
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm.next_standalone_task_serial = u64::MAX;
    let before = vm.heap.stats();
    assert_eq!(
        vm.abort_signal_timeout(&[Value::Number(1.0)]),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.browser_tasks.capacity(), 0);
    assert_eq!(vm.next_standalone_task_serial, u64::MAX);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    let after = vm.heap.stats();
    assert_eq!(
        after.nursery_objects + after.tenured_objects,
        before.nursery_objects + before.tenured_objects
    );
}
#[cfg_attr(test, test)]
fn browser_task_storage_completion_delivery_releases_the_wait_root() {
    let mut fixture = Fixture::new(Producer::Completion, 0);
    fixture.append().unwrap();
    assert!(fixture
        .vm
        .host_completion_queued_for_test(fixture.token.unwrap()));
    assert_eq!(fixture.vm.run_host_task_turn(0, 1).unwrap().tasks_run, 1);
    assert!(fixture.vm.host_promises.pending.is_empty());
    assert!(fixture.vm.browser_tasks.is_empty());
}
#[cfg_attr(test, test)]
fn browser_task_storage_terminal_refusal_preserves_existing_queues() {
    for producer in PRODUCERS {
        let mut fixture = Fixture::new(producer, 2);
        let roots = fixture.vm.heap.root_storage_state();
        let next = serial(&fixture.vm);
        fixture.vm.host_scheduling.as_ref().unwrap().fail();
        assert_eq!(fixture.append(), Err(RuntimeError::SchedulingLimit));
        assert_eq!(
            (
                fixture.vm.browser_tasks.len(),
                fixture.vm.browser_tasks.capacity()
            ),
            (2, 4)
        );
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(serial(&fixture.vm), next);
        fixture.assert_unpublished();
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_browser_task_storage_contracts() {
        browser_task_storage_drag_refuses_net_only_replacement_allowance();
        browser_task_storage_reader_root_refusal_preserves_serial_and_queue_capacity();
        browser_task_storage_timeout_managed_refusal_preserves_serial_and_queue_capacity();
        browser_task_storage_all_producers_refuse_net_only_growth();
        browser_task_storage_full_growth_allowance_refunds_old_backing_and_reader_copy();
        browser_task_storage_one_byte_short_growth_refunds_all_preparations();
        browser_task_storage_spare_slots_admit_only_new_owners();
        browser_task_storage_serial_refusal_preserves_every_producer_state();
        browser_task_storage_shared_count_refusal_preserves_every_producer_state();
        browser_task_storage_root_id_refusal_preserves_rooted_producers();
        browser_task_storage_joint_root_refusal_preserves_queue_and_reader_state();
        browser_task_storage_exact_joint_root_allowance_transfers_each_owner_once();
        browser_task_storage_vm_drop_refunds_each_committed_producer();
        browser_task_storage_rust_held_candidate_keeps_its_document_lease_after_vm_drop();
        browser_task_storage_drag_payload_exact_allowance_transfers_owned_text();
        browser_task_storage_completion_payload_refusal_keeps_the_wait_unqueued();
        browser_task_storage_reader_managed_refusal_preserves_initial_state();
        browser_task_storage_reader_listener_copy_exact_allowance_refunds_nested_owners();
        browser_task_storage_reader_listener_copy_refusal_preserves_original_backing();
        browser_task_storage_reader_epoch_refusal_precedes_task_admission();
        browser_task_storage_dropped_prepared_reader_keeps_the_original_state();
        browser_task_storage_committed_reader_update_retains_a_young_blob_through_minor_gc();
        browser_task_storage_reader_preparation_protects_receiver_and_incoming_edges_during_gc();
        browser_task_storage_reader_start_and_abort_release_the_committed_task();
        browser_task_storage_timeout_serial_refusal_leaves_unpublished_signal_collectible();
        browser_task_storage_completion_delivery_releases_the_wait_root();
        browser_task_storage_terminal_refusal_preserves_existing_queues();
    }
}
