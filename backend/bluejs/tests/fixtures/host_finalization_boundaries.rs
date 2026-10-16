// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn execute(vm: &mut Vm, source: &str) {
    vm.execute_script(&crate::compile(&crate::parse(source).unwrap()).unwrap())
        .unwrap();
}

#[test]
fn document_task_delivery_does_not_enter_an_adopted_foreign_document() {
    let mut owner = Vm::default();
    owner.use_bounded_host_microtask_checkpoints().unwrap();
    let mut foreign = Vm::default();
    foreign.use_bounded_host_microtask_checkpoints().unwrap();
    execute(
        &mut foreign,
        "var cleaned=0,registry=new FinalizationRegistry(()=>cleaned++);registry.register({},0);",
    );
    let record = ShadowRealmRecord::new(foreign);
    owner
        .shadow_realms
        .insert(owner.object_prototype, record.clone());
    assert!(owner.run_browser_task_turn(64).unwrap());
    assert_eq!(
        record
            .vm
            .borrow()
            .host_scheduling_snapshot()
            .pending_browser_tasks,
        1
    );
    assert!(record.vm.borrow_mut().run_browser_task_turn(64).unwrap());
    assert_eq!(
        record
            .vm
            .borrow()
            .host_scheduling_snapshot()
            .pending_browser_tasks,
        0
    );
}

#[test]
fn cleanup_file_reading_and_drag_sources_each_get_a_turn() {
    let mut owner = Vm::default();
    owner.use_bounded_host_microtask_checkpoints().unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    owner
        .install_host_function("record", 1, move |args: &[HostValue]| {
            output.borrow_mut().push(args[0].clone());
            Ok(HostValue::Undefined)
        })
        .unwrap();
    execute(&mut owner, "var registry=new FinalizationRegistry(()=>{});registry.register({},0);registry.register({},1);");
    let global = owner.global("globalThis").unwrap().object_id().unwrap();
    let callback = owner
        .heap
        .get_own(global, "record")
        .unwrap()
        .unwrap()
        .object_id()
        .unwrap();
    owner
        .queue_drag_string_task(callback, "root".into())
        .unwrap();
    let mut child = Vm::default();
    child.bounded_host_microtask_checkpoints = true;
    child.host_scheduling = owner.host_scheduling.as_ref().map(SchedulingScope::child);
    child.install_web_file_api().unwrap();
    let output = log.clone();
    child
        .install_host_function("record", 1, move |args: &[HostValue]| {
            output.borrow_mut().push(args[0].clone());
            Ok(HostValue::Undefined)
        })
        .unwrap();
    execute(
        &mut child,
        "var reader=new FileReader();reader.onloadstart=()=>record('file');reader.readAsText(new Blob(['payload']));",
    );
    owner
        .shadow_realms
        .insert(owner.object_prototype, ShadowRealmRecord::new(child));
    assert!(!owner.run_browser_task_turn(2).unwrap());
    assert_eq!(*log.borrow(), [HostValue::String("file".into())]);
    assert!(!owner.run_browser_task_turn(1).unwrap());
    assert_eq!(
        *log.borrow(),
        [
            HostValue::String("file".into()),
            HostValue::String("root".into())
        ]
    );
    assert!(owner.run_browser_task_turn(64).unwrap());
}
