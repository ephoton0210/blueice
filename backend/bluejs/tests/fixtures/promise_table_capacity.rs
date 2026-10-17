// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admission with tiny real Promise owners and scalar document-budget models.

use super::*;

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
    // No allocation of the represented shared-owner bytes is performed.
    vm.host_scheduling
        .as_ref()
        .unwrap()
        .shared_charge(MAX_HOST_SCHEDULING_BYTES - used - bytes)
        .unwrap()
}

fn map_growth<K: Eq + std::hash::Hash, V>(map: &HashMap<K, V>) -> usize {
    let replacement = HashMap::<K, V>::with_capacity(map.len() + 1);
    replacement.capacity().saturating_sub(map.capacity()) * std::mem::size_of::<(K, V)>()
}

#[cfg_attr(test, test)]
fn promise_table_growth_is_refused_before_allocating_the_promise() {
    let mut vm = page_vm();
    execute(
        &mut vm,
        "var kept=[]; for(var i=0;i<7;i++) kept.push(Promise.resolve(i)); 0;",
    );
    assert_eq!(vm.promises.len(), vm.promises.capacity());
    let capacity = vm.promises.capacity();
    let growth = map_growth(&vm.promises);
    let _owner = leave_room(&mut vm, growth - 1);
    let heap = vm.heap.stats();
    assert_eq!(vm.new_promise(), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.promises.len(), 7);
    assert_eq!(vm.promises.capacity(), capacity);
    assert_eq!(vm.heap.stats(), heap);
}

#[cfg_attr(test, test)]
fn promise_table_existing_slots_remain_usable() {
    let mut vm = page_vm();
    execute(
        &mut vm,
        "var kept=[]; for(var i=0;i<5;i++) kept.push(Promise.resolve(i)); 0;",
    );
    let capacity = vm.promises.capacity();
    assert!(capacity > vm.promises.len());
    let _owner = leave_room(&mut vm, 0);
    vm.new_promise().unwrap();
    assert_eq!(vm.synchronize_scheduling_usage(), Ok(()));
    assert_eq!(vm.promises.len(), 6);
    assert_eq!(vm.promises.capacity(), capacity);
}

#[cfg_attr(test, test)]
fn script_promise_refusal_is_terminal_without_publishing_another_record() {
    let mut vm = page_vm();
    execute(
        &mut vm,
        "var kept=[]; for(var i=0;i<7;i++) kept.push(Promise.resolve(i)); 0;",
    );
    let capacity = vm.promises.capacity();
    let bytes = map_growth(&vm.promises);
    let _owner = leave_room(&mut vm, bytes - 1);
    let code =
        crate::compile(&crate::parse("try { Promise.resolve(8); } catch(error) {} 1;").unwrap())
            .unwrap();
    assert_eq!(vm.execute_script(&code), Err(RuntimeError::SchedulingLimit));
    assert_eq!(vm.promises.len(), 7);
    assert_eq!(vm.promises.capacity(), capacity);
}

fn waits(entries: usize) -> (Vm, Rc<RefCell<Vec<HostPromiseToken>>>) {
    let mut vm = page_vm();
    let starts = Rc::new(RefCell::new(Vec::new()));
    let output = starts.clone();
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
    execute(
        &mut vm,
        &format!("for(var i=0;i<{entries};i++) request(); 0;"),
    );
    (vm, starts)
}

fn assert_wait_refused(
    mut vm: Vm,
    starts: Rc<RefCell<Vec<HostPromiseToken>>>,
    bytes: usize,
    args: &[Value],
) {
    let count = starts.borrow().len();
    let promises = (vm.promises.len(), vm.promises.capacity());
    let waits = (
        vm.host_promises.pending.len(),
        vm.host_promises.pending.capacity(),
    );
    let serial = vm
        .host_scheduling
        .as_ref()
        .unwrap()
        .account
        .borrow()
        .next_task_serial;
    let _owner = leave_room(&mut vm, bytes);
    let heap = vm.heap.stats();
    assert_eq!(
        vm.host_async_function_call(0, Value::Undefined, args, false),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(starts.borrow().len(), count);
    assert_eq!((vm.promises.len(), vm.promises.capacity()), promises);
    assert_eq!(
        (
            vm.host_promises.pending.len(),
            vm.host_promises.pending.capacity()
        ),
        waits
    );
    assert_eq!(
        vm.host_scheduling
            .as_ref()
            .unwrap()
            .account
            .borrow()
            .next_task_serial,
        serial
    );
    assert_eq!(vm.heap.stats(), heap);
}

#[cfg_attr(test, test)]
fn host_wait_table_growth_is_refused_before_capabilities_or_service_start() {
    let (vm, starts) = waits(7);
    assert_eq!(
        vm.host_promises.pending.len(),
        vm.host_promises.pending.capacity()
    );
    let growth = map_growth(&vm.host_promises.pending) + map_growth(&vm.promises);
    assert_wait_refused(vm, starts, growth - 1, &[]);
}

#[cfg_attr(test, test)]
fn host_wait_existing_slots_remain_usable() {
    let (mut vm, starts) = waits(5);
    let capacity = vm.host_promises.pending.capacity();
    assert!(capacity > vm.host_promises.pending.len());
    let _owner = leave_room(&mut vm, 0);
    vm.host_async_function_call(0, Value::Undefined, &[], false)
        .unwrap();
    assert_eq!(starts.borrow().len(), 6);
    assert_eq!(vm.host_promises.pending.len(), 6);
    assert_eq!(vm.host_promises.pending.capacity(), capacity);
}

#[cfg_attr(test, test)]
fn host_wait_admits_argument_storage_and_promise_growth_together() {
    let (mut vm, starts) = waits(5);
    execute(
        &mut vm,
        "var kept=[Promise.resolve(1),Promise.resolve(2)]; 0;",
    );
    assert_eq!(vm.promises.len(), vm.promises.capacity());
    let bytes = map_growth(&vm.promises) + std::mem::size_of::<HostValue>();
    assert_wait_refused(vm, starts, bytes - 1, &[Value::Number(1.0)]);
}

#[cfg_attr(test, test)]
fn host_wait_root_refusal_refunds_argument_leases_and_reconciles_capacity() {
    let (mut vm, starts) = waits(0);
    execute(&mut vm, "Promise; 0;");
    vm.heap.allow_root_registrations(0);
    assert_eq!(
        vm.host_async_function_call(
            0,
            Value::Undefined,
            &[Value::String("payload".into())],
            false
        ),
        Err(RuntimeError::Heap(HeapError::IdExhausted))
    );
    assert!(starts.borrow().is_empty());
    assert!(vm.host_promises.pending.is_empty());
    assert_eq!(vm.displaced_execution_bytes.get(), 0);
    assert_eq!(vm.active_scheduling_bytes, 0);
    assert_eq!(
        vm.host_scheduling
            .as_ref()
            .unwrap()
            .account
            .borrow()
            .usage
            .payload_bytes,
        vm.own_scheduling_usage().payload_bytes
            + vm.heap.collector_storage_bytes()
            + vm.heap.root_storage_bytes()
    );
}

enum Frame {
    Async(Box<AsyncContinuation>),
    Module(Box<ModuleContinuation>),
}

struct AwaitFixture {
    vm: Vm,
    input: ObjectId,
    frame: Option<Frame>,
    asynchronous: bool,
}

impl AwaitFixture {
    fn new(asynchronous: bool, retained: usize) -> Self {
        let mut vm = page_vm();
        execute(
            &mut vm,
            "var input=new Promise(()=>{}), outputs=[]; function callback() {} 0;",
        );
        if asynchronous {
            execute(&mut vm, &format!("async function wait() {{ await input; return 1; }} for(var i=0;i<{};i++) outputs.push(wait()); 0;", retained + 1));
        } else {
            for index in 0..=retained {
                let entry = format!("table-{index}.mjs");
                let code = crate::compile_module(
                    &crate::parse_module("await input; export const value = 1;").unwrap(),
                )
                .unwrap();
                vm.execute_module_graph(&entry, &HashMap::from([(entry.clone(), code)]))
                    .unwrap();
            }
        }
        let input = vm
            .lookup_global_name("input")
            .unwrap()
            .unwrap()
            .object_id()
            .unwrap();
        let (serial, frame) = if asynchronous {
            assert_eq!(vm.async_continuations.len(), retained + 1);
            let serial = *vm.async_continuations.keys().max().unwrap();
            let frame = vm.async_continuations.remove(&serial).unwrap();
            vm.async_continuations.shrink_to_fit();
            (serial, Frame::Async(Box::new(frame)))
        } else {
            assert_eq!(vm.module_continuations.len(), retained + 1);
            let serial = *vm.module_continuations.keys().max().unwrap();
            let frame = vm.module_continuations.remove(&serial).unwrap();
            vm.module_continuations.shrink_to_fit();
            (serial, Frame::Module(Box::new(frame)))
        };
        vm.promises.get_mut(&input).unwrap().reactions.retain(|reaction| !matches!(reaction,
            PromiseReaction::AsyncAwait { continuation } if asynchronous && *continuation == serial)
            && !matches!(reaction, PromiseReaction::ModuleAwait { continuation } if !asynchronous && *continuation == serial));
        Self {
            vm,
            input,
            frame: Some(frame),
            asynchronous,
        }
    }

    fn frame_bytes(&self) -> usize {
        match self.frame.as_ref().unwrap() {
            Frame::Async(frame) => frame.scheduling_payload_bytes(),
            Frame::Module(frame) => frame.scheduling_payload_bytes(),
        }
    }

    fn table(&self) -> (usize, usize, u64) {
        if self.asynchronous {
            (
                self.vm.async_continuations.len(),
                self.vm.async_continuations.capacity(),
                self.vm.next_async_continuation,
            )
        } else {
            (
                self.vm.module_continuations.len(),
                self.vm.module_continuations.capacity(),
                self.vm.next_module_continuation,
            )
        }
    }

    fn growth(&self) -> usize {
        if self.asynchronous {
            map_growth(&self.vm.async_continuations)
        } else {
            map_growth(&self.vm.module_continuations)
        }
    }

    fn register(&mut self) -> Result<(), RuntimeError> {
        match self.frame.take().unwrap() {
            Frame::Async(frame) => self.vm.suspend_async_await(*frame, self.input),
            Frame::Module(frame) => self.vm.suspend_module_await(*frame, self.input),
        }
    }

    fn assert_refused(&mut self) {
        let table = self.table();
        let reactions = self.vm.promises[&self.input].reactions.len();
        let jobs = self.vm.promise_jobs.len();
        assert_eq!(self.register(), Err(RuntimeError::SchedulingLimit));
        assert_eq!(self.table(), table);
        assert_eq!(self.vm.promises[&self.input].reactions.len(), reactions);
        assert_eq!(self.vm.promise_jobs.len(), jobs);
    }
}

fn await_map_growth(asynchronous: bool) {
    let mut fixture = AwaitFixture::new(asynchronous, 7);
    let bytes = fixture.frame_bytes() + fixture.growth();
    let _owner = leave_room(&mut fixture.vm, bytes - 1);
    fixture.assert_refused();
}

fn await_existing_slot(asynchronous: bool) {
    let mut fixture = AwaitFixture::new(asynchronous, 5);
    let (_, capacity, _) = fixture.table();
    let bytes = fixture.frame_bytes();
    let _owner = leave_room(&mut fixture.vm, bytes);
    assert_eq!(fixture.register(), Ok(()));
    assert_eq!(fixture.table().0, 6);
    assert_eq!(fixture.table().1, capacity);
}

fn await_reaction_count(asynchronous: bool) {
    let mut fixture = AwaitFixture::new(asynchronous, 5);
    let count = fixture
        .vm
        .host_scheduling_usage()
        .unwrap()
        .pending_reactions;
    let mut peer = fixture.vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_reactions: MAX_PENDING_HOST_REACTIONS - count,
            ..Default::default()
        },
        HostSchedulingUsage::default(),
    )
    .unwrap();
    fixture.assert_refused();
}

fn await_fulfilled_job_count(asynchronous: bool) {
    let mut fixture = AwaitFixture::new(asynchronous, 5);
    fixture
        .vm
        .settle_promise(fixture.input, PromiseStatus::Fulfilled(Value::Number(0.0)))
        .unwrap();
    let count = fixture
        .vm
        .host_scheduling_usage()
        .unwrap()
        .pending_microtasks;
    let mut peer = fixture.vm.host_scheduling.as_ref().unwrap().child();
    peer.check(
        HostSchedulingUsage {
            pending_microtasks: MAX_PENDING_HOST_MICROTASKS - count,
            ..Default::default()
        },
        HostSchedulingUsage::default(),
    )
    .unwrap();
    fixture.assert_refused();
}

fn await_rejected_job_growth(asynchronous: bool) {
    let mut fixture = AwaitFixture::new(asynchronous, 5);
    fixture
        .vm
        .settle_promise(fixture.input, PromiseStatus::Rejected(Value::Number(0.0)))
        .unwrap();
    let callback = fixture.vm.lookup_global_name("callback").unwrap().unwrap();
    for _ in 0..3 {
        fixture.vm.enqueue_promise_job(PromiseJob::HostMicrotask {
            callback: callback.clone(),
        });
    }
    assert_eq!(
        fixture.vm.promise_jobs.len(),
        fixture.vm.promise_jobs.capacity()
    );
    let bytes = fixture.frame_bytes()
        + fixture.vm.promise_jobs.capacity() * std::mem::size_of::<PromiseJob>();
    let _owner = leave_room(&mut fixture.vm, bytes - 1);
    fixture.assert_refused();
}

macro_rules! await_tests {
    ($(($name:ident, $helper:ident, $asynchronous:expr)),* $(,)?) => {
        $( #[cfg_attr(test, test)] fn $name() { $helper($asynchronous); } )*
        fn verify_await_contracts() { $( $name(); )* }
    };
}

await_tests!(
    (
        async_table_growth_precedes_frame_publication,
        await_map_growth,
        true
    ),
    (
        module_table_growth_precedes_frame_publication,
        await_map_growth,
        false
    ),
    (
        async_existing_frame_slot_is_usable,
        await_existing_slot,
        true
    ),
    (
        module_existing_frame_slot_is_usable,
        await_existing_slot,
        false
    ),
    (
        async_reaction_refusal_precedes_frame_publication,
        await_reaction_count,
        true
    ),
    (
        module_reaction_refusal_precedes_frame_publication,
        await_reaction_count,
        false
    ),
    (
        async_fulfilled_job_refusal_precedes_frame_publication,
        await_fulfilled_job_count,
        true
    ),
    (
        module_fulfilled_job_refusal_precedes_frame_publication,
        await_fulfilled_job_count,
        false
    ),
    (
        async_rejected_job_growth_precedes_frame_publication,
        await_rejected_job_growth,
        true
    ),
    (
        module_rejected_job_growth_precedes_frame_publication,
        await_rejected_job_growth,
        false
    ),
);

impl crate::Vm {
    /// Runs Promise table admission contracts in unit and coverage builds.
    #[doc(hidden)]
    pub fn verify_promise_table_capacity_contracts() {
        promise_table_growth_is_refused_before_allocating_the_promise();
        promise_table_existing_slots_remain_usable();
        script_promise_refusal_is_terminal_without_publishing_another_record();
        host_wait_table_growth_is_refused_before_capabilities_or_service_start();
        host_wait_existing_slots_remain_usable();
        host_wait_admits_argument_storage_and_promise_growth_together();
        host_wait_root_refusal_refunds_argument_leases_and_reconciles_capacity();
        verify_await_contracts();
    }
}
