// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, HostFileSelectionEvent, HostObjectKey, HostValue, Value, Vm, VmConfig,
};
fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}
fn vm() -> Vm {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    config.heap.major_threshold_bytes = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_web_file_api().unwrap();
    vm
}

#[test]
fn host_click_and_file_listeners_accept_signals_and_detach_during_native_dispatch() {
    let mut vm = vm();
    let family = vm.create_host_object_family().unwrap();
    let key = HostObjectKey::new(7, 2, 11);
    vm.install_host_object_factory("node", 0, family, move |_: &[HostValue]| Ok(Some(key)))
        .unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        const node1=node(), controller=new AbortController();var events=[];
        node1.addEventListener('click',()=>{events.push('first');controller.abort();},{once:true});
        for(const name of ['click','input','change'])node1.addEventListener(name,()=>events.push(name),{signal:controller.signal});
        true
    "#
        ),
        Value::Bool(true)
    );
    assert!(!vm.dispatch_host_click(family, key).unwrap());
    vm.dispatch_host_file_selection_event(family, &[key], HostFileSelectionEvent::Input)
        .unwrap();
    vm.dispatch_host_file_selection_event(family, &[key], HostFileSelectionEvent::Change)
        .unwrap();
    assert_eq!(
        run(&mut vm, "events.join(',') === 'first'"),
        Value::Bool(true)
    );
}

#[test]
fn weak_dependency_edges_do_not_retain_unobserved_or_source_less_signals() {
    let mut vm = vm();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var weakSource,weakDependent,weakUnused;const live=new AbortController();
        (()=>{const source=new AbortController(), dependent=AbortSignal.any([source.signal]);
            dependent.onabort=()=>{};weakSource=new WeakRef(source.signal);weakDependent=new WeakRef(dependent);
            weakUnused=new WeakRef(AbortSignal.any([live.signal]));})();
        true
    "#
        ),
        Value::Bool(true)
    );
    run(
        &mut vm,
        "for(let index=0;index<100;index++){new EventTarget();} true",
    );
    assert_eq!(run(&mut vm,"weakSource.deref() === undefined && weakDependent.deref() === undefined && weakUnused.deref() === undefined"),Value::Bool(true));
}

#[test]
fn positive_timeout_yields_without_blocking_and_keeps_dependent_observers_alive() {
    let mut vm = vm();
    assert_eq!(
        run(
            &mut vm,
            r#"
        var report='';(()=>{const timeout=AbortSignal.timeout(25), combined=AbortSignal.any([timeout]);
            combined.onabort=()=>{report=combined.reason.name;};})();true
    "#
        ),
        Value::Bool(true)
    );
    assert!(!vm.run_browser_task_turn(64).unwrap());
    std::thread::sleep(std::time::Duration::from_millis(40));
    assert!(vm.run_browser_task_turn(64).unwrap());
    assert_eq!(run(&mut vm, "report === 'TimeoutError'"), Value::Bool(true));
}

#[test]
fn aborting_listener_registration_does_not_cancel_file_reader_data_work() {
    let mut vm = vm();
    assert_eq!(
        run(
            &mut vm,
            r#"
        const controller=new AbortController(), reader=new FileReader();var count=0;
        reader.addEventListener('load',()=>count++,{signal:controller.signal});
        reader.readAsText(new Blob(['complete 中文']));controller.abort();true
    "#
        ),
        Value::Bool(true)
    );
    vm.run_browser_tasks_bounded(64).unwrap();
    assert_eq!(
        run(
            &mut vm,
            "reader.readyState === 2 && reader.result === 'complete 中文' && count === 0"
        ),
        Value::Bool(true)
    );
}

#[test]
fn abort_during_dispatch_removes_pending_listeners_and_once_detaches_its_hook() {
    let mut vm = vm();
    assert_eq!(
        run(
            &mut vm,
            r#"
        const target=new EventTarget(), controller=new AbortController(), events=[];
        target.addEventListener('x',()=>{events.push('first');controller.abort();});
        target.addEventListener('x',()=>events.push('cancelled'),{signal:controller.signal});
        target.dispatchEvent(new Event('x'));
        const onceController=new AbortController();let count=0;
        target.addEventListener('once',()=>count++,{once:true,signal:onceController.signal});
        target.dispatchEvent(new Event('once'));onceController.abort();target.dispatchEvent(new Event('once'));
        events.join(',')==='first' && count===1
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn signal_sequence_conversion_closes_invalid_iterators_and_checks_every_value() {
    let mut vm = vm();
    assert_eq!(
        run(
            &mut vm,
            r#"
        let closed=0, rejected=false;
        const input={ [Symbol.iterator]() {return {next(){return {value:{},done:false};},return(){closed++;return {};}};} };
        try{AbortSignal.any(input);}catch(error){rejected=error instanceof TypeError;}
        const a=new AbortController(), b=AbortSignal.abort('first aborted');
        const signal=AbortSignal.any(new Set([a.signal,b]));a.abort('late');
        rejected && closed===1 && signal.reason==='first aborted'
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn null_listener_signals_fail_brand_conversion_and_removed_callbacks_do_not_stay_rooted() {
    let mut vm = vm();
    assert_eq!(
        run(
            &mut vm,
            r#"
        const controller=new AbortController();var weakTarget;let invalid=false;
        try{new EventTarget().addEventListener('x',()=>{}, {signal:null});}catch(error){invalid=error instanceof TypeError;}
        (()=>{const target=new EventTarget(), callback=()=>{};weakTarget=new WeakRef(target);
            target.addEventListener('x',callback,{signal:controller.signal});target.removeEventListener('x',callback);})();
        invalid
    "#
        ),
        Value::Bool(true)
    );
    run(
        &mut vm,
        "for(let index=0;index<100;index++){new EventTarget();} true",
    );
    assert_eq!(
        run(&mut vm, "weakTarget.deref() === undefined"),
        Value::Bool(true)
    );
}
