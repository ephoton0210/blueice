// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed continuation awaits preserve ownership until native preparation commits.

use super::*;
use crate::vm::host_scheduling::SharedSchedulingCharge;

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.use_bounded_host_microtask_checkpoints().unwrap();
    vm.global("Promise").unwrap();
    vm.remaining_instructions = vm.config.instruction_budget;
    vm
}
fn execute(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap()
}
fn leave_room(vm: &mut Vm, bytes: usize) -> SharedSchedulingCharge {
    let used = vm.host_scheduling_usage().unwrap().payload_bytes;
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}
fn input(vm: &mut Vm) -> ObjectId {
    vm.lookup_global_name("input")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap()
}
fn reject_for_retry(vm: &mut Vm, promise: ObjectId) {
    reject_value_for_retry(vm, promise, Value::Number(7.0));
}
fn reject_value_for_retry(vm: &mut Vm, promise: ObjectId, value: Value) {
    vm.promises.get_mut(&promise).unwrap().reactions = Vec::new();
    vm.promises.get_mut(&promise).unwrap().handled = false;
    vm.host_rejections = Some(host_rejections::HostRejections::default());
    vm.settle_promise(promise, PromiseStatus::Rejected(value))
        .unwrap();
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.len(), 1);
}
enum Frame {
    Async(Box<AsyncContinuation>),
    Module(Box<ModuleContinuation>),
}
fn capture_frame(asynchronous: bool) -> (Vm, ObjectId, Frame) {
    let mut vm = page();
    execute(&mut vm, "globalThis.input=new Promise(()=>{});0;");
    let frame = if asynchronous {
        execute(&mut vm, "globalThis.output=(async()=>await input)();0;");
        let id = *vm.async_continuations.keys().next().unwrap();
        Frame::Async(Box::new(vm.async_continuations.remove(&id).unwrap()))
    } else {
        let code = crate::compile_module(
            &crate::parse_module("await input;export const value=1;").unwrap(),
        )
        .unwrap();
        vm.execute_module_graph("entry", &HashMap::from([("entry".into(), code)]))
            .unwrap();
        let id = *vm.module_continuations.keys().next().unwrap();
        Frame::Module(Box::new(vm.module_continuations.remove(&id).unwrap()))
    };
    let promise = input(&mut vm);
    vm.promises.get_mut(&promise).unwrap().reactions = Vec::new();
    vm.promises.get_mut(&promise).unwrap().handled = false;
    (vm, promise, frame)
}
fn frame_fixture(asynchronous: bool) -> (Vm, ObjectId, Frame) {
    let (mut vm, promise, frame) = capture_frame(asynchronous);
    reject_for_retry(&mut vm, promise);
    (vm, promise, frame)
}
fn register_frame(vm: &mut Vm, promise: ObjectId, frame: Frame) -> Result<(), RuntimeError> {
    match frame {
        Frame::Async(frame) => vm.suspend_async_await(*frame, promise),
        Frame::Module(frame) => vm.suspend_module_await(*frame, promise),
    }
}
fn retained(vm: &Vm, promise: ObjectId, roots: (usize, usize, u64)) {
    assert!(!vm.promises[&promise].handled);
    assert_eq!(vm.host_rejections.as_ref().unwrap().pending.len(), 1);
    assert_eq!(vm.heap.root_storage_state(), roots);
    assert!(vm.promise_jobs.is_empty());
}
#[cfg_attr(test, test)]
fn await_registration_frame_refusal_preserves_rejection_and_serials() {
    for asynchronous in [false, true] {
        let (mut vm, promise, frame) = frame_fixture(asynchronous);
        let serials = (vm.next_async_continuation, vm.next_module_continuation);
        let roots = vm.heap.root_storage_state();
        let _peer = leave_room(&mut vm, 0);
        let result = register_frame(&mut vm, promise, frame);
        retained(&vm, promise, roots);
        assert_eq!(result, Err(RuntimeError::SchedulingLimit));
        assert_eq!(
            (vm.next_async_continuation, vm.next_module_continuation),
            serials
        );
        assert!(vm.async_continuations.is_empty() && vm.module_continuations.is_empty());
    }
}
fn capture_generator(yielding: bool) -> (Vm, ObjectId, ObjectId, ObjectId, Option<ObjectId>) {
    let mut vm = page();
    execute(&mut vm, "globalThis.input=new Promise(()=>{});0;");
    if yielding {
        // Completed-generator return uses the AsyncGeneratorYield await helper.
        execute(
            &mut vm,
            "globalThis.gen=(async function*(){})();globalThis.request=gen.return(input);0;",
        );
    } else {
        execute(
            &mut vm,
            "globalThis.gen=(async function*(){yield 0;})();globalThis.request=gen.next();0;",
        );
        vm.run_host_microtask_checkpoint().unwrap();
        execute(&mut vm, "globalThis.request=gen.return(input);0;");
    }
    let promise = input(&mut vm);
    let reaction = vm
        .promises
        .get_mut(&promise)
        .unwrap()
        .reactions
        .pop()
        .unwrap();
    let (generator, target, result) = match reaction {
        PromiseReaction::AsyncGeneratorYield {
            generator,
            target,
            result,
        } => (generator, target, Some(result)),
        PromiseReaction::AsyncGeneratorDelegate {
            generator,
            target,
            kind: AsyncGeneratorDelegateKind::AwaitReturn,
        } => (generator, target, None),
        _ => panic!("expected actual suspended generator await"),
    };
    vm.stack.reserve(12);
    (vm, promise, generator, target, result)
}
fn generator_fixture(yielding: bool) -> (Vm, ObjectId, ObjectId, ObjectId, Option<ObjectId>) {
    let (mut vm, promise, generator, target, result) = capture_generator(yielding);
    reject_for_retry(&mut vm, promise);
    (vm, promise, generator, target, result)
}
#[cfg_attr(test, test)]
fn await_registration_generator_yield_refusal_keeps_rejection_root() {
    let (mut vm, promise, generator, target, result) = generator_fixture(true);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    let outcome = vm.await_async_generator_yield(generator, target, Value::Object(result.unwrap()));
    retained(&vm, promise, roots);
    assert_eq!(outcome, Err(RuntimeError::SchedulingLimit));
    assert!(vm.stack.is_empty());
}
#[cfg_attr(test, test)]
fn await_registration_generator_return_refusal_keeps_rejection_root() {
    let (mut vm, promise, generator, target, _) = generator_fixture(false);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    let outcome = vm.start_async_generator_await(
        generator,
        target,
        AsyncGeneratorDelegateKind::AwaitReturn,
        Value::Object(promise),
    );
    retained(&vm, promise, roots);
    assert_eq!(outcome, Err(RuntimeError::SchedulingLimit));
    assert!(vm.stack.is_empty());
}

impl Frame {
    fn bytes(&self) -> usize {
        match self {
            Self::Async(frame) => frame.scheduling_payload_bytes(),
            Self::Module(frame) => frame.scheduling_payload_bytes(),
        }
    }
}
fn serials(vm: &Vm) -> (u64, u64) {
    (vm.next_async_continuation, vm.next_module_continuation)
}
fn origin() -> ThrowOrigin {
    ThrowOrigin {
        source: Some(crate::diagnostics::DiagnosticSourceLocation {
            filename: "tiny.js".into(),
            line: 3,
            column: 5,
        }),
        epoch: 7,
        ..Default::default()
    }
}
fn descriptors(id: ObjectId) -> [AwaitContinuation; 4] {
    [
        AwaitContinuation::Module { continuation: 31 },
        AwaitContinuation::Async { continuation: 37 },
        AwaitContinuation::GeneratorYield {
            generator: id,
            target: id,
            result: id,
        },
        AwaitContinuation::GeneratorDelegate {
            generator: id,
            target: id,
            kind: AsyncGeneratorDelegateKind::Return,
        },
    ]
}
fn scalar_source(settled: Option<bool>, tracking: bool) -> (Vm, ObjectId) {
    let mut vm = page();
    let promise = vm.new_promise().unwrap();
    if tracking {
        vm.host_rejections = Some(host_rejections::HostRejections::default());
    }
    vm.promise_origin_override = Some(origin());
    if let Some(fulfilled) = settled {
        let value = Value::String("tiny".into());
        vm.settle_promise(
            promise,
            if fulfilled {
                PromiseStatus::Fulfilled(value)
            } else {
                PromiseStatus::Rejected(value)
            },
        )
        .unwrap();
    }
    (vm, promise)
}
fn observer(vm: &Vm) -> Vm {
    Vm {
        host_scheduling: Some(vm.host_scheduling.as_ref().unwrap().child()),
        ..Vm::default()
    }
}

#[cfg_attr(test, test)]
fn await_registration_frame_serial_refusal_precedes_handled_changes() {
    for asynchronous in [false, true] {
        let (mut vm, promise, frame) = frame_fixture(asynchronous);
        if asynchronous {
            vm.next_async_continuation = u64::MAX;
        } else {
            vm.next_module_continuation = u64::MAX;
        }
        let before = serials(&vm);
        let roots = vm.heap.root_storage_state();
        assert_eq!(
            register_frame(&mut vm, promise, frame),
            Err(RuntimeError::InstructionLimit)
        );
        retained(&vm, promise, roots);
        assert_eq!(serials(&vm), before);
    }
}
#[cfg_attr(test, test)]
fn await_registration_frame_map_refusal_refunds_prepared_queue_without_handling() {
    for asynchronous in [false, true] {
        let (mut vm, promise, frame) = frame_fixture(asynchronous);
        let map_min = if asynchronous {
            vm.async_continuations.release_empty();
            std::mem::size_of::<(u64, AsyncContinuation)>()
        } else {
            vm.module_continuations.release_empty();
            std::mem::size_of::<(u64, ModuleContinuation)>()
        };
        let room = frame.bytes() + 4 * std::mem::size_of::<PromiseJob>() + map_min - 1;
        let before = serials(&vm);
        let roots = vm.heap.root_storage_state();
        let _peer = leave_room(&mut vm, room);
        assert_eq!(
            register_frame(&mut vm, promise, frame),
            Err(RuntimeError::SchedulingLimit)
        );
        retained(&vm, promise, roots);
        assert_eq!(serials(&vm), before);
        assert_eq!(vm.promise_jobs.capacity(), 0);
        if asynchronous {
            assert_eq!(vm.async_continuations.capacity(), 0);
        } else {
            assert_eq!(vm.module_continuations.capacity(), 0);
        }
    }
}
#[cfg_attr(test, test)]
fn await_registration_successful_frames_commit_owner_job_serial_then_handling() {
    for asynchronous in [false, true] {
        let (mut vm, promise, frame) = frame_fixture(asynchronous);
        let before = serials(&vm);
        let roots = vm.heap.root_storage_state();
        register_frame(&mut vm, promise, frame).unwrap();
        assert!(vm.promises[&promise].handled);
        assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
        assert_eq!(vm.promise_jobs.len(), 1);
        assert_eq!(vm.heap.root_storage_state().0, roots.0 - 1);
        assert_eq!(vm.heap.root_storage_state().2, roots.2);
        assert_eq!(
            serials(&vm),
            (
                before.0 + u64::from(asynchronous),
                before.1 + u64::from(!asynchronous)
            )
        );
        assert_eq!(
            vm.async_continuations.len() + vm.module_continuations.len(),
            1
        );
    }
}
#[cfg_attr(test, test)]
fn await_registration_frame_job_quota_refusal_preserves_notification_and_fuel() {
    for asynchronous in [false, true] {
        let (mut vm, promise, frame) = frame_fixture(asynchronous);
        let roots = vm.heap.root_storage_state();
        let before = serials(&vm);
        let mut peer = vm.host_scheduling.as_ref().unwrap().child();
        peer.check(
            HostSchedulingUsage {
                pending_microtasks: MAX_PENDING_HOST_MICROTASKS,
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap();
        vm.remaining_instructions = 0;
        assert_eq!(
            register_frame(&mut vm, promise, frame),
            Err(RuntimeError::SchedulingLimit)
        );
        retained(&vm, promise, roots);
        assert_eq!(serials(&vm), before);
        assert_eq!(vm.remaining_instructions, 0);
    }
}
#[cfg_attr(test, test)]
fn await_registration_pending_frames_publish_their_reaction_before_handling() {
    for asynchronous in [false, true] {
        let (mut vm, promise, frame) = capture_frame(asynchronous);
        register_frame(&mut vm, promise, frame).unwrap();
        assert!(vm.promises[&promise].handled);
        assert_eq!(vm.promises[&promise].reactions.len(), 1);
        assert!(vm.promise_jobs.is_empty());
        assert_eq!(
            vm.async_continuations.len() + vm.module_continuations.len(),
            1
        );
        assert!(vm.check_scheduling_failure().is_ok());
    }
}
#[cfg_attr(test, test)]
fn await_registration_all_pending_descriptors_preserve_scalar_metadata() {
    for index in 0..4 {
        let (mut vm, promise) = scalar_source(None, false);
        let prepared = vm
            .prepare_continuation_await(promise, descriptors(promise)[index], 0)
            .unwrap();
        assert!(!vm.promises[&promise].handled);
        prepared.publish_and_handle(&mut vm, promise).unwrap();
        assert!(vm.promises[&promise].handled);
        assert_eq!(vm.promises[&promise].reactions.len(), 1);
        let reaction = &vm.promises[&promise].reactions[0];
        assert!(match (index, reaction) {
            (0, PromiseReaction::ModuleAwait { continuation: 31 }) => true,
            (1, PromiseReaction::AsyncAwait { continuation: 37 }) => true,
            (
                2,
                PromiseReaction::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                },
            ) => *generator == promise && *target == promise && *result == promise,
            (
                3,
                PromiseReaction::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind: AsyncGeneratorDelegateKind::Return,
                },
            ) => *generator == promise && *target == promise,
            _ => false,
        });
    }
}
#[cfg_attr(test, test)]
fn await_registration_all_settled_descriptors_move_value_and_origin_once() {
    for fulfilled in [false, true] {
        for index in 0..4 {
            let (mut vm, promise) = scalar_source(Some(fulfilled), false);
            let roots = vm.heap.root_storage_state();
            let work = vm
                .prepare_continuation_await(promise, descriptors(promise)[index], 0)
                .unwrap();
            work.publish_and_handle(&mut vm, promise).unwrap();
            assert_eq!(vm.promise_jobs.len(), 1);
            let (value, outcome, copied) = match &vm.promise_jobs[0] {
                PromiseJob::ModuleAwait {
                    value,
                    fulfilled,
                    origin,
                    ..
                }
                | PromiseJob::AsyncAwait {
                    value,
                    fulfilled,
                    origin,
                    ..
                }
                | PromiseJob::AsyncGeneratorYield {
                    value,
                    fulfilled,
                    origin,
                    ..
                }
                | PromiseJob::AsyncGeneratorDelegate {
                    value,
                    fulfilled,
                    origin,
                    ..
                } => (value, *fulfilled, origin),
                _ => panic!("expected continuation job"),
            };
            assert_eq!(value, &Value::String("tiny".into()));
            assert_eq!(outcome, fulfilled);
            assert_eq!(copied.epoch, if fulfilled { 0 } else { 7 });
            assert_eq!(vm.heap.root_storage_state(), roots);
        }
    }
}
#[cfg_attr(test, test)]
fn await_registration_short_copy_allowance_preserves_rejection_before_growth() {
    let (mut vm, promise) = scalar_source(Some(false), true);
    let roots = vm.heap.root_storage_state();
    let descriptor = descriptors(promise)[3];
    let _peer = leave_room(&mut vm, 8 + "tiny.js".len() - 1);
    assert!(matches!(
        vm.prepare_continuation_await(promise, descriptor, 0),
        Err(RuntimeError::SchedulingLimit)
    ));
    retained(&vm, promise, roots);
    assert_eq!(vm.promise_jobs.capacity(), 0);
}
#[cfg_attr(test, test)]
fn await_registration_pending_descriptor_quota_refuses_before_queue_mutation() {
    for index in 0..4 {
        let (mut vm, promise) = scalar_source(None, false);
        let mut peer = vm.host_scheduling.as_ref().unwrap().child();
        peer.check(
            HostSchedulingUsage {
                pending_reactions: MAX_PENDING_HOST_REACTIONS,
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap();
        assert!(matches!(
            vm.prepare_continuation_await(promise, descriptors(promise)[index], 0),
            Err(RuntimeError::SchedulingLimit)
        ));
        assert!(!vm.promises[&promise].handled);
        assert_eq!(vm.promises[&promise].reactions.capacity(), 0);
    }
}
#[cfg_attr(test, test)]
fn await_registration_settled_descriptor_quota_keeps_notification_roots() {
    for index in 0..4 {
        let (mut vm, promise) = scalar_source(Some(false), true);
        let roots = vm.heap.root_storage_state();
        let mut peer = vm.host_scheduling.as_ref().unwrap().child();
        peer.check(
            HostSchedulingUsage {
                pending_microtasks: MAX_PENDING_HOST_MICROTASKS,
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap();
        assert!(matches!(
            vm.prepare_continuation_await(promise, descriptors(promise)[index], 0),
            Err(RuntimeError::SchedulingLimit)
        ));
        retained(&vm, promise, roots);
    }
}
#[cfg_attr(test, test)]
fn await_registration_exact_spare_job_slot_adopts_its_input_without_double_charge() {
    let (mut vm, promise) = scalar_source(Some(true), false);
    vm.enqueue_promise_job(PromiseJob::HostMicrotask {
        callback: Value::Undefined,
    });
    let _peer = leave_room(&mut vm, 8);
    let work = vm
        .prepare_continuation_await(promise, descriptors(promise)[1], 0)
        .unwrap();
    work.publish_and_handle(&mut vm, promise).unwrap();
    assert_eq!(vm.promise_jobs.len(), 2);
    assert_eq!(vm.promise_jobs.capacity(), 4);
    assert_eq!(
        vm.host_scheduling_usage().unwrap().payload_bytes,
        MAX_HOST_SCHEDULING_BYTES
    );
}
#[cfg_attr(test, test)]
fn await_registration_prepared_native_ownership_survives_vm_destruction() {
    let (mut vm, promise) = scalar_source(Some(false), false);
    let view = observer(&vm);
    let before = vm.host_scheduling_usage().unwrap().payload_bytes;
    let work = vm
        .prepare_continuation_await(promise, descriptors(promise)[2], 0)
        .unwrap();
    let extra = vm.host_scheduling_snapshot().payload_bytes - before;
    drop(vm);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, extra);
    drop(work);
    assert_eq!(view.host_scheduling_snapshot().payload_bytes, 0);
}
#[cfg_attr(test, test)]
fn await_registration_abandoned_preparation_refunds_without_notification_changes() {
    let (mut vm, promise) = scalar_source(Some(false), true);
    let before = vm.host_scheduling_usage().unwrap();
    let roots = vm.heap.root_storage_state();
    let work = vm
        .prepare_continuation_await(promise, descriptors(promise)[0], 0)
        .unwrap();
    drop(work);
    retained(&vm, promise, roots);
    assert_eq!(vm.host_scheduling_usage().unwrap(), before);
}
#[cfg_attr(test, test)]
fn await_registration_scheduler_resource_refusal_keeps_the_original_return_request() {
    let (mut vm, promise, generator, target, _) = generator_fixture(false);
    vm.set_async_generator_status(generator, AsyncGeneratorStatus::SuspendedYield)
        .unwrap();
    let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    let request = control.requests.front().unwrap().clone();
    let request_id = request.id;
    drop(control);
    let roots = vm.heap.root_storage_state();
    let _peer = leave_room(&mut vm, 0);
    assert_eq!(
        vm.resume_async_generator_request(generator, AsyncGeneratorStatus::SuspendedYield, request),
        Err(RuntimeError::SchedulingLimit)
    );
    retained(&vm, promise, roots);
    let control = vm.heap.async_generator_control(generator).unwrap().unwrap();
    assert_eq!(control.requests.len(), 1);
    assert_eq!(control.requests[0].id, request_id);
    assert_eq!(control.requests[0].target, target);
    assert!(
        matches!(control.requests[0].completion, AsyncGeneratorCompletion::Return(Value::Object(id)) if id == promise)
    );
    assert!(matches!(
        control.status,
        AsyncGeneratorStatus::SuspendedYield
    ));
}
#[cfg_attr(test, test)]
fn await_registration_generator_yield_success_delivers_original_rejection() {
    let (mut vm, promise, generator, target, result) = generator_fixture(true);
    vm.await_async_generator_yield(generator, target, Value::Object(result.unwrap()))
        .unwrap();
    assert!(vm.promises[&promise].handled);
    assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(matches!(
        vm.promises[&target].status,
        PromiseStatus::Rejected(Value::Number(7.0))
    ));
    assert!(vm
        .heap
        .async_generator_control(generator)
        .unwrap()
        .unwrap()
        .requests
        .is_empty());
}
#[cfg_attr(test, test)]
fn await_registration_generator_return_success_delivers_original_rejection() {
    let (mut vm, promise, generator, target, _) = generator_fixture(false);
    vm.start_async_generator_await(
        generator,
        target,
        AsyncGeneratorDelegateKind::AwaitReturn,
        Value::Object(promise),
    )
    .unwrap();
    assert!(vm.promises[&promise].handled);
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(matches!(
        vm.promises[&target].status,
        PromiseStatus::Rejected(Value::Number(7.0))
    ));
    assert!(vm
        .heap
        .async_generator_control(generator)
        .unwrap()
        .unwrap()
        .requests
        .is_empty());
}
#[cfg_attr(test, test)]
fn await_registration_public_async_and_module_completion_keep_job_boundaries() {
    let mut vm = page();
    execute(&mut vm, "globalThis.trace=[];globalThis.resolve=null;const p=new Promise(r=>{resolve=r});globalThis.output=(async()=>{trace.push('a');await p;trace.push('b');return 3})();trace.push('sync');resolve(2);0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "trace.join(',')"),
        Value::String("a,sync,b".into())
    );
    let code = crate::compile_module(
        &crate::parse_module(
            "await Promise.resolve(1);globalThis.trace.push('module');export const value=42;",
        )
        .unwrap(),
    )
    .unwrap();
    vm.execute_module_graph("entry", &HashMap::from([("entry".into(), code)]))
        .unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "trace.join(',')"),
        Value::String("a,sync,b,module".into())
    );
    assert!(vm.async_continuations.is_empty() && vm.module_continuations.is_empty());
}
#[cfg_attr(test, test)]
fn await_registration_public_delegate_return_and_throw_preserve_results() {
    let mut vm = page();
    execute(&mut vm, "globalThis.trace=[];const it={ [Symbol.asyncIterator](){return this},next(){return {value:0,done:false}},return(){trace.push('return');return Promise.resolve({value:4,done:true})},throw(e){trace.push('throw');return Promise.resolve({value:e,done:true})}};async function* g(){return yield* it;}globalThis.gen=g();gen.next();0;");
    vm.run_host_microtask_checkpoint().unwrap();
    execute(&mut vm, "gen.return(5).then(r=>trace.push('r'+r.value));0;");
    vm.run_host_microtask_checkpoint().unwrap();
    execute(&mut vm, "globalThis.second=g();second.next();0;");
    vm.run_host_microtask_checkpoint().unwrap();
    execute(
        &mut vm,
        "second.throw(9).then(r=>trace.push('t'+r.value));0;",
    );
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "trace.join(',')"),
        Value::String("return,r4,throw,t9".into())
    );
}
#[cfg_attr(test, test)]
fn await_registration_public_rejected_yield_and_return_cross_catch_and_finally() {
    let mut vm = page();
    execute(&mut vm, "globalThis.trace=[];async function* g(){try{yield Promise.reject('bad')}catch(e){trace.push(e);yield 1}finally{trace.push('finally')}}globalThis.gen=g();gen.next().then(r=>trace.push('v'+r.value));0;");
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "trace.join(',')"),
        Value::String("bad,v1".into())
    );
    execute(
        &mut vm,
        "gen.return(Promise.reject('stop')).catch(e=>trace.push(e));0;",
    );
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        execute(&mut vm, "trace.join(',')"),
        Value::String("bad,v1,finally,stop".into())
    );
}

#[cfg_attr(test, test)]
fn await_registration_generator_job_and_notification_roots_release_after_delivery() {
    let (mut vm, promise, generator, target, result) = capture_generator(true);
    let result = result.unwrap();
    let reason = vm.with_roots(|heap| heap.alloc_object(None)).unwrap();
    reject_value_for_retry(&mut vm, promise, Value::Object(reason));
    vm.await_async_generator_yield(generator, target, Value::Object(result))
        .unwrap();
    assert!(vm.host_rejections.as_ref().unwrap().pending.is_empty());
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    for name in ["input", "gen", "request"] {
        vm.heap.set(global, name, Value::Null).unwrap();
    }
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!([promise, generator, target, result, reason]
        .iter()
        .all(|id| vm.heap.contains(*id)));
    vm.run_host_microtask_checkpoint().unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!([promise, generator, result]
        .iter()
        .all(|id| !vm.heap.contains(*id)));
    assert!(vm.heap.contains(target) && vm.heap.contains(reason));
    vm.mark_promise_handled(target);
    vm.run_browser_task_turn(1).unwrap();
    vm.with_roots(|heap| heap.try_collect_major()).unwrap();
    assert!(!vm.heap.contains(target) && !vm.heap.contains(reason));
}

#[cfg_attr(test, test)]
fn await_registration_late_notification_failure_retains_committed_frame_and_work() {
    for asynchronous in [false, true] {
        let (mut vm, promise, frame) = frame_fixture(asynchronous);
        vm.mark_promise_handled(promise);
        let record = vm.promises.get_mut(&promise).unwrap();
        record.handled = false;
        record.rejection_notified = true;
        vm.heap.allow_root_registrations(0);
        let roots = vm.heap.root_storage_state();
        let before = serials(&vm);
        assert_eq!(
            register_frame(&mut vm, promise, frame),
            Err(RuntimeError::SchedulingLimit)
        );
        assert!(vm.promises[&promise].handled);
        assert!(vm.promises[&promise].rejection_notified);
        assert_eq!(vm.promise_jobs.len(), 1);
        assert_eq!(
            vm.async_continuations.len() + vm.module_continuations.len(),
            1
        );
        assert_eq!(
            serials(&vm),
            (
                before.0 + u64::from(asynchronous),
                before.1 + u64::from(!asynchronous)
            )
        );
        assert_eq!(vm.heap.root_storage_state(), roots);
        assert!(vm.browser_tasks.is_empty());
    }
}

impl Vm {
    #[doc(hidden)]
    pub fn verify_await_registration_contracts() {
        await_registration_frame_refusal_preserves_rejection_and_serials();
        await_registration_generator_yield_refusal_keeps_rejection_root();
        await_registration_generator_return_refusal_keeps_rejection_root();
        await_registration_frame_serial_refusal_precedes_handled_changes();
        await_registration_frame_map_refusal_refunds_prepared_queue_without_handling();
        await_registration_successful_frames_commit_owner_job_serial_then_handling();
        await_registration_frame_job_quota_refusal_preserves_notification_and_fuel();
        await_registration_pending_frames_publish_their_reaction_before_handling();
        await_registration_all_pending_descriptors_preserve_scalar_metadata();
        await_registration_all_settled_descriptors_move_value_and_origin_once();
        await_registration_short_copy_allowance_preserves_rejection_before_growth();
        await_registration_pending_descriptor_quota_refuses_before_queue_mutation();
        await_registration_settled_descriptor_quota_keeps_notification_roots();
        await_registration_exact_spare_job_slot_adopts_its_input_without_double_charge();
        await_registration_prepared_native_ownership_survives_vm_destruction();
        await_registration_abandoned_preparation_refunds_without_notification_changes();
        await_registration_scheduler_resource_refusal_keeps_the_original_return_request();
        await_registration_generator_yield_success_delivers_original_rejection();
        await_registration_generator_return_success_delivers_original_rejection();
        await_registration_public_async_and_module_completion_keep_job_boundaries();
        await_registration_public_delegate_return_and_throw_preserve_results();
        await_registration_public_rejected_yield_and_return_cross_catch_and_finally();
        await_registration_generator_job_and_notification_roots_release_after_delivery();
        await_registration_late_notification_failure_retains_committed_frame_and_work();
    }
}
