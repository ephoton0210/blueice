// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tiny rooted queues and scalar allowances verify preparation and commitment.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page_vm() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm
}
fn leave_room(vm: &mut Vm, room: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - room)
        .unwrap()
}
fn serial(vm: &Vm) -> u64 {
    vm.host_scheduling.as_ref().unwrap().task_serial_state()
}
fn fill_roots(vm: &mut Vm, free: usize) {
    assert!(vm.heap.root_storage_state().1 <= 63);
    // Reusing a tombstone can increase reported insertion capacity without a
    // new allocation. Shape against each current value, with a fixed small cap.
    for _ in 0..128 {
        let (len, capacity, _) = vm.heap.root_storage_state();
        assert!(capacity <= 127);
        if capacity - len == free {
            vm.synchronize_scheduling_usage().unwrap();
            return;
        }
        vm.heap.root(vm.object_prototype).unwrap();
    }
    panic!("bounded root fixture did not reach its required insertion headroom");
}
fn frame_fixture(count: usize) -> (Vm, Value) {
    let mut vm = page_vm();
    vm.install_host_function("callback", 1, |_args: &[HostValue]| {
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    for _ in 0..count {
        request(&mut vm, callback.clone()).unwrap();
    }
    (vm, callback)
}
fn request(vm: &mut Vm, callback: Value) -> Result<Value, RuntimeError> {
    vm.host_animation_frame_call(
        host_animation_frames::HostAnimationFrameFunction::Request,
        &[callback],
        false,
    )
}
fn cleanup_fixture(count: usize) -> (Vm, ObjectId) {
    let mut vm = page_vm();
    vm.install_host_function("callback", 1, |_args: &[HostValue]| {
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    let mut last = None;
    for index in 0..=count {
        let registry = vm
            .heap
            .alloc_finalization_registry(callback.clone(), None)
            .unwrap();
        vm.heap.root(registry).unwrap();
        let target = vm.heap.alloc_object(None).unwrap();
        vm.heap
            .finalization_registry_register(
                registry,
                Value::Object(target),
                Value::Number(index as f64),
                None,
            )
            .unwrap();
        if index == count {
            last = Some((registry, vm.heap.root(target).unwrap()));
        }
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm.enqueue_host_finalization_tasks().unwrap();
    assert_eq!(vm.host_finalization_tasks.len(), count);
    let (registry, target_root) = last.unwrap();
    vm.heap.unroot(target_root).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    (vm, registry)
}
fn rejection_fixture(count: usize) -> (Vm, ObjectId) {
    let mut vm = page_vm();
    vm.host_rejections = Some(Default::default());
    let mut last = None;
    for index in 0..=count {
        let promise = vm.new_promise().unwrap();
        vm.heap.root(promise).unwrap();
        vm.promises.get_mut(&promise).unwrap().status =
            PromiseStatus::Rejected(Value::Number(index as f64));
        if index < count {
            vm.track_host_rejection(promise);
        } else {
            last = Some(promise);
        }
    }
    vm.synchronize_scheduling_usage().unwrap();
    (vm, last.unwrap())
}

#[cfg_attr(test, test)]
fn rooted_task_storage_frame_refuses_net_only_replacement_allowance() {
    let (mut vm, callback) = frame_fixture(4);
    assert_eq!(vm.host_animation_frames.capacity(), 4);
    fill_roots(&mut vm, 1);
    let roots = vm.heap.root_storage_state();
    let next = vm.host_animation_frame_next_id;
    let _peer = leave_room(&mut vm, 4 * std::mem::size_of::<HostAnimationFrame>());
    assert_eq!(
        request(&mut vm, callback),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_animation_frames.len(), 4);
    assert_eq!(vm.host_animation_frames.capacity(), 4);
    assert_eq!(vm.host_animation_frame_next_id, next);
    assert_eq!(vm.heap.root_storage_state(), roots);
}
#[cfg_attr(test, test)]
fn rooted_task_storage_cleanup_root_refusal_preserves_serial_and_queue() {
    let (mut vm, _) = cleanup_fixture(0);
    vm.heap.allow_root_registrations(0);
    let roots = vm.heap.root_storage_state();
    let next = serial(&vm);
    assert_eq!(
        vm.enqueue_host_finalization_tasks(),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert_eq!(serial(&vm), next);
    assert_eq!(vm.host_finalization_tasks.capacity(), 0);
    assert_eq!(vm.heap.root_storage_state(), roots);
}
#[cfg_attr(test, test)]
fn rooted_task_storage_rejection_root_refusal_preserves_pending_capacity() {
    let (mut vm, promise) = rejection_fixture(0);
    vm.heap.allow_root_registrations(0);
    let roots = vm.heap.root_storage_state();
    vm.track_host_rejection(promise);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.capacity(), 0);
    assert_eq!(vm.heap.root_storage_state(), roots);
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Frame,
    Cleanup,
    Rejection,
}
const KINDS: [Kind; 3] = [Kind::Frame, Kind::Cleanup, Kind::Rejection];
struct Fixture {
    vm: Vm,
    target: Value,
    kind: Kind,
}
impl Fixture {
    fn new(kind: Kind, count: usize) -> Self {
        let (vm, target) = match kind {
            Kind::Frame => frame_fixture(count),
            Kind::Cleanup => {
                let (vm, target) = cleanup_fixture(count);
                (vm, Value::Object(target))
            }
            Kind::Rejection => {
                let (vm, target) = rejection_fixture(count);
                (vm, Value::Object(target))
            }
        };
        Self { vm, target, kind }
    }
    fn append(&mut self) -> Result<(), RuntimeError> {
        match self.kind {
            Kind::Frame => request(&mut self.vm, self.target.clone()).map(|_| ()),
            Kind::Cleanup => self.vm.enqueue_host_finalization_tasks(),
            Kind::Rejection => {
                self.vm
                    .track_host_rejection(self.target.object_id().unwrap());
                self.vm.check_scheduling_failure()
            }
        }
    }
    fn state(&self) -> (usize, usize) {
        match self.kind {
            Kind::Frame => (
                self.vm.host_animation_frames.len(),
                self.vm.host_animation_frames.capacity(),
            ),
            Kind::Cleanup => (
                self.vm.host_finalization_tasks.len(),
                self.vm.host_finalization_tasks.capacity(),
            ),
            Kind::Rejection => {
                let pending = &self.vm.host_rejections.as_ref().unwrap().pending;
                (pending.len(), pending.capacity())
            }
        }
    }
    fn identity(&self) -> u64 {
        match self.kind {
            Kind::Frame => self.vm.host_animation_frame_next_id,
            _ => serial(&self.vm),
        }
    }
    fn bytes(&self, capacity: usize) -> usize {
        capacity
            * match self.kind {
                Kind::Frame => std::mem::size_of::<HostAnimationFrame>(),
                Kind::Cleanup => std::mem::size_of::<host_finalization::HostFinalizationTask>(),
                Kind::Rejection => std::mem::size_of::<host_rejections::PendingRejection>(),
            }
    }
    fn scratch_bytes(&self) -> usize {
        match self.kind {
            Kind::Cleanup => self
                .vm
                .prepare_finalization_registry_snapshot()
                .unwrap()
                .retained_bytes(),
            _ => 0,
        }
    }
}
fn root_candidate_bytes(vm: &Vm) -> usize {
    let (len, capacity, _) = vm.heap.root_storage_state();
    assert_eq!(len, capacity);
    host_scheduling::map_slots(&HashMap::<RootId, ObjectId>::with_capacity(
        (len + 1).max(len * 2),
    ))
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}
fn clear_callback_property(vm: &mut Vm) {
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.heap.set(global, "callback", Value::Undefined).unwrap();
    assert_eq!(vm.heap.get(global, "callback").unwrap(), Value::Undefined);
}

#[cfg_attr(test, test)]
fn rooted_task_storage_complete_growth_commits_once_and_refunds_old_backing() {
    for kind in KINDS {
        let mut fixture = Fixture::new(kind, 4);
        fill_roots(&mut fixture.vm, 1);
        let scratch = fixture.scratch_bytes();
        let old = fixture.bytes(4);
        let room = scratch + fixture.bytes(8);
        let roots = fixture.vm.heap.root_storage_state().0;
        let next = fixture.identity();
        let _peer = leave_room(&mut fixture.vm, room);
        fixture.append().unwrap();
        assert_eq!(fixture.state(), (5, 8), "{kind:?}");
        assert_eq!(fixture.vm.heap.root_storage_state().0, roots + 1);
        assert_eq!(
            fixture.identity(),
            next + u64::from(!matches!(kind, Kind::Rejection))
        );
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - scratch - old,
            "{kind:?}"
        );
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_one_byte_short_growth_preserves_all_native_owners() {
    for kind in KINDS {
        let mut fixture = Fixture::new(kind, 4);
        fill_roots(&mut fixture.vm, 1);
        let room = fixture.scratch_bytes() + fixture.bytes(8) - 1;
        let roots = fixture.vm.heap.root_storage_state();
        let next = fixture.identity();
        let before = fixture.vm.heap.stats();
        let _peer = leave_room(&mut fixture.vm, room);
        assert_eq!(
            fixture.append(),
            Err(RuntimeError::SchedulingLimit),
            "{kind:?}"
        );
        assert_eq!(fixture.state(), (4, 4));
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(fixture.vm.heap.stats(), before);
        assert_eq!(fixture.identity(), next);
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - room
        );
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_root_storage_refusal_keeps_initial_queue_unpublished() {
    for kind in KINDS {
        let mut fixture = Fixture::new(kind, 0);
        fill_roots(&mut fixture.vm, 0);
        let room = fixture.scratch_bytes() + fixture.bytes(4);
        let roots = fixture.vm.heap.root_storage_state();
        let next = fixture.identity();
        let _peer = leave_room(&mut fixture.vm, room);
        assert_eq!(
            fixture.append(),
            Err(RuntimeError::SchedulingLimit),
            "{kind:?}"
        );
        assert_eq!(fixture.state(), (0, 0));
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(fixture.identity(), next);
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - room
        );
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_exact_joint_root_allowance_refunds_displaced_root_storage() {
    for kind in KINDS {
        let mut fixture = Fixture::new(kind, 0);
        fill_roots(&mut fixture.vm, 0);
        let scratch = fixture.scratch_bytes();
        let old = fixture.vm.heap.root_storage_bytes();
        let room = scratch + fixture.bytes(4) + root_candidate_bytes(&fixture.vm);
        let _peer = leave_room(&mut fixture.vm, room);
        fixture.append().unwrap();
        assert_eq!(fixture.state(), (1, 4), "{kind:?}");
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - scratch - old,
            "{kind:?}"
        );
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_one_byte_short_joint_root_allowance_refunds_every_candidate() {
    for kind in KINDS {
        let mut fixture = Fixture::new(kind, 0);
        fill_roots(&mut fixture.vm, 0);
        let room =
            fixture.scratch_bytes() + fixture.bytes(4) + root_candidate_bytes(&fixture.vm) - 1;
        let roots = fixture.vm.heap.root_storage_state();
        let next = fixture.identity();
        let _peer = leave_room(&mut fixture.vm, room);
        assert_eq!(
            fixture.append(),
            Err(RuntimeError::SchedulingLimit),
            "{kind:?}"
        );
        assert_eq!(fixture.state(), (0, 0));
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(fixture.identity(), next);
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - room
        );
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_existing_slots_need_only_cleanup_snapshot_storage() {
    for kind in KINDS {
        let mut fixture = Fixture::new(kind, 2);
        fill_roots(&mut fixture.vm, 1);
        let scratch = fixture.scratch_bytes();
        let _peer = leave_room(&mut fixture.vm, scratch);
        fixture.append().unwrap();
        assert_eq!(fixture.state(), (3, 4), "{kind:?}");
        assert_eq!(
            fixture.vm.host_scheduling_snapshot().payload_bytes,
            MAX_HOST_SCHEDULING_BYTES - scratch
        );
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_shared_counts_refuse_without_publishing_roots_or_capacity() {
    for kind in [Kind::Cleanup, Kind::Rejection] {
        let mut fixture = Fixture::new(kind, 0);
        let mut peer = fixture.vm.host_scheduling.as_ref().unwrap().child();
        peer.check(
            match kind {
                Kind::Cleanup => HostSchedulingUsage {
                    pending_browser_tasks: MAX_PENDING_BROWSER_TASKS,
                    ..Default::default()
                },
                _ => HostSchedulingUsage {
                    pending_reactions: MAX_PENDING_HOST_REACTIONS,
                    ..Default::default()
                },
            },
            HostSchedulingUsage::default(),
        )
        .unwrap();
        let roots = fixture.vm.heap.root_storage_state();
        let next = fixture.identity();
        assert_eq!(fixture.append(), Err(RuntimeError::SchedulingLimit));
        assert_eq!(fixture.state(), (0, 0));
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(fixture.identity(), next);
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_identifier_refusal_preserves_initial_queue_and_ownership() {
    for kind in KINDS {
        let mut fixture = Fixture::new(kind, 0);
        match kind {
            Kind::Frame => fixture.vm.host_animation_frame_next_id = u64::from(u32::MAX) + 1,
            Kind::Cleanup => fixture
                .vm
                .host_scheduling
                .as_ref()
                .unwrap()
                .set_task_serial_for_test(u64::MAX),
            Kind::Rejection => fixture.vm.heap.set_root_counter_for_test(u64::MAX),
        }
        let roots = fixture.vm.heap.root_storage_state();
        let next = fixture.identity();
        let before = fixture.vm.host_scheduling_usage().unwrap().payload_bytes;
        let result = fixture.append();
        if matches!(kind, Kind::Frame) {
            assert!(matches!(result, Err(RuntimeError::RangeError(_))));
            assert_eq!(fixture.vm.check_scheduling_failure(), Ok(()));
        } else {
            assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        }
        assert_eq!(fixture.state(), (0, 0));
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(fixture.identity(), next);
        assert_eq!(fixture.vm.host_scheduling_snapshot().payload_bytes, before);
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_terminal_refusal_preserves_existing_entries() {
    for kind in KINDS {
        let mut fixture = Fixture::new(kind, 2);
        let roots = fixture.vm.heap.root_storage_state();
        let next = fixture.identity();
        let before = fixture.vm.host_scheduling_usage().unwrap().payload_bytes;
        fixture.vm.host_scheduling.as_ref().unwrap().fail();
        assert_eq!(fixture.append(), Err(RuntimeError::SchedulingLimit));
        assert_eq!(fixture.state(), (2, 4));
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(fixture.identity(), next);
        assert_eq!(fixture.vm.host_scheduling_snapshot().payload_bytes, before);
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_vm_drop_refunds_committed_queues_and_roots() {
    for kind in KINDS {
        let fixture = Fixture::new(kind, 2);
        let observed = observer(&fixture.vm);
        drop(fixture);
        assert_eq!(
            observed.host_scheduling_snapshot().payload_bytes,
            0,
            "{kind:?}"
        );
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_cleanup_duplicate_uses_only_snapshot_storage() {
    let (mut vm, _) = cleanup_fixture(0);
    vm.enqueue_host_finalization_tasks().unwrap();
    let bytes = vm
        .prepare_finalization_registry_snapshot()
        .unwrap()
        .retained_bytes();
    let roots = vm.heap.root_storage_state();
    let next = serial(&vm);
    let _peer = leave_room(&mut vm, bytes);
    vm.enqueue_host_finalization_tasks().unwrap();
    assert_eq!(vm.host_finalization_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(serial(&vm), next);
    assert_eq!(
        vm.host_scheduling_snapshot().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES - bytes
    );
}
#[cfg_attr(test, test)]
fn rooted_task_storage_cleanup_root_survives_gc_and_releases_after_delivery() {
    let (mut vm, callback) = frame_fixture(0);
    let registry = vm.heap.alloc_finalization_registry(callback, None).unwrap();
    let keep = vm.heap.root(registry).unwrap();
    let target = vm.heap.alloc_object(None).unwrap();
    vm.heap
        .finalization_registry_register(registry, Value::Object(target), Value::Number(1.0), None)
        .unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    vm.enqueue_host_finalization_tasks().unwrap();
    vm.heap.unroot(keep).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(registry));
    vm.run_next_host_finalization_task().unwrap();
    assert!(vm.host_finalization_tasks.is_empty());
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(registry));
}
#[cfg_attr(test, test)]
fn rooted_task_storage_rejection_handling_releases_root_and_preserves_retained_slots() {
    let (mut vm, promise) = rejection_fixture(0);
    vm.track_host_rejection(promise);
    let roots = vm.heap.root_storage_state().0;
    vm.mark_promise_handled(promise);
    assert_eq!(vm.heap.root_storage_state().0, roots - 1);
    let pending = &vm.host_rejections.as_ref().unwrap().pending;
    assert!(pending.is_empty());
    assert_eq!(pending.capacity(), 4);
    assert_eq!(vm.host_scheduling_usage().unwrap().pending_reactions, 0);
    assert!(vm.promises[&promise].handled);
}
#[cfg_attr(test, test)]
fn rooted_task_storage_rejection_notification_moves_existing_vector_ownership() {
    let (mut vm, _) = rejection_fixture(1);
    let roots = vm.heap.root_storage_state();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.notify_host_rejections().unwrap();
    let pending = &vm.host_rejections.as_ref().unwrap().pending;
    assert!(pending.is_empty());
    assert_eq!(pending.capacity(), 0);
    assert_eq!(vm.browser_tasks.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    let usage = vm.host_scheduling_usage().unwrap();
    assert_eq!(usage.pending_reactions, 1);
    assert_eq!(
        usage.payload_bytes,
        before + 4 * std::mem::size_of::<browser_tasks::BrowserTask>()
    );
}
#[cfg_attr(test, test)]
fn rooted_task_storage_frame_callback_survives_gc_and_releases_after_dispatch() {
    let mut vm = page_vm();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let output = observed.clone();
    vm.install_host_function("callback", 1, move |args: &[HostValue]| {
        output.borrow_mut().push(args[0].clone());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let callback = vm.lookup_global_name("callback").unwrap().unwrap();
    let object = callback.object_id().unwrap();
    request(&mut vm, callback).unwrap();
    clear_callback_property(&mut vm);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(object));
    let run = vm.run_host_animation_frame(7.0).unwrap();
    assert_eq!(run.callbacks_run, 1);
    assert_eq!(run.uncaught_errors, 0);
    assert_eq!(*observed.borrow(), [HostValue::Number(7.0)]);
    assert!(vm.host_animation_frames.is_empty());
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
}
#[cfg_attr(test, test)]
fn rooted_task_storage_frame_cancellation_releases_root_and_last_queue_slots() {
    let (mut vm, callback) = frame_fixture(1);
    let object = callback.object_id().unwrap();
    clear_callback_property(&mut vm);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(vm.heap.contains(object));
    let roots = vm.heap.root_storage_state().0;
    vm.host_animation_frame_call(
        host_animation_frames::HostAnimationFrameFunction::Cancel,
        &[Value::Number(1.0)],
        false,
    )
    .unwrap();
    assert!(vm.host_animation_frames.is_empty());
    assert_eq!(vm.host_animation_frames.capacity(), 0);
    assert_eq!(vm.heap.root_storage_state().0, roots - 1);
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(object));
}
#[cfg_attr(test, test)]
fn rooted_task_storage_invalid_frame_inputs_preserve_roots_and_handles() {
    let (mut vm, callback) = frame_fixture(0);
    let roots = vm.heap.root_storage_state();
    assert!(matches!(
        request(&mut vm, Value::Undefined),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        vm.host_animation_frame_call(
            host_animation_frames::HostAnimationFrameFunction::Request,
            &[callback],
            true
        ),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(vm.host_animation_frame_next_id, 1);
    assert_eq!(vm.host_animation_frames.capacity(), 0);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.check_scheduling_failure(), Ok(()));
}
#[cfg_attr(test, test)]
fn rooted_task_storage_cleanup_and_rejection_refuse_net_only_growth() {
    for kind in [Kind::Cleanup, Kind::Rejection] {
        let mut fixture = Fixture::new(kind, 4);
        fill_roots(&mut fixture.vm, 1);
        let room = fixture.scratch_bytes() + fixture.bytes(4);
        let roots = fixture.vm.heap.root_storage_state();
        let next = fixture.identity();
        let _peer = leave_room(&mut fixture.vm, room);
        assert_eq!(fixture.append(), Err(RuntimeError::SchedulingLimit));
        assert_eq!(fixture.state(), (4, 4));
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(fixture.identity(), next);
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_root_id_refusal_preserves_admitted_spare_slots() {
    for kind in KINDS {
        let mut fixture = Fixture::new(kind, 2);
        fixture.vm.heap.allow_root_registrations(0);
        let roots = fixture.vm.heap.root_storage_state();
        let next = fixture.identity();
        let before = fixture.vm.host_scheduling_usage().unwrap().payload_bytes;
        let expected = if matches!(kind, Kind::Rejection) {
            RuntimeError::SchedulingLimit
        } else {
            RuntimeError::Heap(HeapError::IdExhausted)
        };
        assert_eq!(fixture.append(), Err(expected));
        assert_eq!(fixture.state(), (2, 4));
        assert_eq!(fixture.vm.heap.root_storage_state(), roots);
        assert_eq!(fixture.identity(), next);
        assert_eq!(fixture.vm.host_scheduling_snapshot().payload_bytes, before);
    }
}
#[cfg_attr(test, test)]
fn rooted_task_storage_cleanup_serial_refusal_refunds_queue_and_root_candidates() {
    let (mut vm, _) = cleanup_fixture(0);
    fill_roots(&mut vm, 0);
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .set_task_serial_for_test(u64::MAX);
    let roots = vm.heap.root_storage_state();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert_eq!(
        vm.enqueue_host_finalization_tasks(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.host_finalization_tasks.capacity(), 0);
    assert_eq!(serial(&vm), u64::MAX);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}
#[cfg_attr(test, test)]
fn rooted_task_storage_rejection_foreign_root_refusal_publishes_no_capacity() {
    let (mut vm, _) = rejection_fixture(0);
    let mut foreign = Vm::default();
    let object = foreign.new_promise().unwrap();
    let roots = vm.heap.root_storage_state();
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.track_host_rejection(object);
    assert_eq!(
        vm.check_scheduling_failure(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.capacity(), 0);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert_eq!(vm.host_scheduling_snapshot().payload_bytes, before);
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_rooted_task_storage_contracts() {
        rooted_task_storage_frame_refuses_net_only_replacement_allowance();
        rooted_task_storage_cleanup_root_refusal_preserves_serial_and_queue();
        rooted_task_storage_rejection_root_refusal_preserves_pending_capacity();
        rooted_task_storage_complete_growth_commits_once_and_refunds_old_backing();
        rooted_task_storage_one_byte_short_growth_preserves_all_native_owners();
        rooted_task_storage_root_storage_refusal_keeps_initial_queue_unpublished();
        rooted_task_storage_exact_joint_root_allowance_refunds_displaced_root_storage();
        rooted_task_storage_one_byte_short_joint_root_allowance_refunds_every_candidate();
        rooted_task_storage_existing_slots_need_only_cleanup_snapshot_storage();
        rooted_task_storage_shared_counts_refuse_without_publishing_roots_or_capacity();
        rooted_task_storage_identifier_refusal_preserves_initial_queue_and_ownership();
        rooted_task_storage_terminal_refusal_preserves_existing_entries();
        rooted_task_storage_vm_drop_refunds_committed_queues_and_roots();
        rooted_task_storage_cleanup_duplicate_uses_only_snapshot_storage();
        rooted_task_storage_cleanup_root_survives_gc_and_releases_after_delivery();
        rooted_task_storage_rejection_handling_releases_root_and_preserves_retained_slots();
        rooted_task_storage_rejection_notification_moves_existing_vector_ownership();
        rooted_task_storage_frame_callback_survives_gc_and_releases_after_dispatch();
        rooted_task_storage_frame_cancellation_releases_root_and_last_queue_slots();
        rooted_task_storage_invalid_frame_inputs_preserve_roots_and_handles();
        rooted_task_storage_cleanup_and_rejection_refuse_net_only_growth();
        rooted_task_storage_root_id_refusal_preserves_admitted_spare_slots();
        rooted_task_storage_cleanup_serial_refusal_refunds_queue_and_root_candidates();
        rooted_task_storage_rejection_foreign_root_refusal_publishes_no_capacity();
    }
}
