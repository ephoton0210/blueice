// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}

fn record(vm: &mut Vm, log: &Rc<std::cell::RefCell<Vec<HostValue>>>) -> ObjectId {
    let output = log.clone();
    vm.install_host_function("record", 1, move |args: &[HostValue]| {
        output.borrow_mut().push(args[0].clone());
        Ok(HostValue::Undefined)
    })
    .unwrap();
    let global = vm.global("globalThis").unwrap().object_id().unwrap();
    vm.heap
        .get_own(global, "record")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap()
}

fn attach(owner: &mut Vm, child: Vm) {
    owner
        .shadow_realms
        .insert(owner.object_prototype, ShadowRealmRecord::new(child));
}

fn child(owner: &Vm) -> Vm {
    Vm {
        bounded_host_microtask_checkpoints: true,
        host_scheduling: owner.host_scheduling.as_ref().map(SchedulingScope::child),
        ..Vm::default()
    }
}

#[test]
fn one_logical_source_keeps_enqueue_order_across_document_realms() {
    let mut owner = Vm::default();
    owner.use_bounded_host_microtask_checkpoints();
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    let root_callback = record(&mut owner, &log);
    let mut child = child(&owner);
    let child_callback = record(&mut child, &log);
    child
        .queue_drag_string_task(child_callback, "child".into())
        .unwrap();
    owner
        .queue_drag_string_task(root_callback, "root".into())
        .unwrap();
    attach(&mut owner, child);
    assert_eq!(owner.run_host_task_turn(0, 2).unwrap().tasks_run, 2);
    assert_eq!(
        *log.borrow(),
        [
            HostValue::String("child".into()),
            HostValue::String("root".into())
        ]
    );
}

#[test]
fn same_document_child_timers_share_the_timer_source_fifo() {
    let mut owner = Vm::default();
    owner.use_bounded_host_microtask_checkpoints();
    owner.install_host_timer_functions().unwrap();
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    record(&mut owner, &log);
    let mut child = child(&owner);
    child.install_host_timer_functions().unwrap();
    record(&mut child, &log);
    execute(&mut child, "setTimeout(()=>record('child'),0);");
    execute(&mut owner, "setTimeout(()=>record('root'),0);");
    attach(&mut owner, child);
    assert_eq!(owner.run_host_task_turn(0, 2).unwrap().tasks_run, 2);
    assert_eq!(
        *log.borrow(),
        [
            HostValue::String("child".into()),
            HostValue::String("root".into())
        ]
    );
}

#[test]
fn child_abort_deadlines_use_the_owning_document_clock_and_wake() {
    let mut owner = Vm::default();
    owner.use_bounded_host_microtask_checkpoints();
    assert_eq!(owner.next_host_task_wake(100).unwrap(), None);
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut child = child(&owner);
    child.install_web_file_api().unwrap();
    record(&mut child, &log);
    execute(
        &mut child,
        "var signal=AbortSignal.timeout(10);signal.onabort=()=>record('abort');",
    );
    attach(&mut owner, child);
    assert_eq!(owner.next_host_task_wake(109).unwrap(), Some(1));
    assert_eq!(owner.run_host_task_turn(109, 64).unwrap().tasks_run, 0);
    assert_eq!(owner.run_host_task_turn(110, 64).unwrap().tasks_run, 1);
    assert_eq!(*log.borrow(), [HostValue::String("abort".into())]);
}
