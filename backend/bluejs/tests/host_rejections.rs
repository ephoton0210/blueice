// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

#[test]
fn rejection_tracking_is_owner_selected_and_refuses_colliding_globals() {
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    assert_eq!(run(&mut vm,"typeof PromiseRejectionEvent+','+typeof onunhandledrejection+','+typeof onrejectionhandled").unwrap(),Value::String("undefined,undefined,undefined".into()));
    vm.install_host_rejection_tracking().unwrap();
    assert!(vm.install_host_rejection_tracking().is_err());
    for name in [
        "PromiseRejectionEvent",
        "onunhandledrejection",
        "onrejectionhandled",
    ] {
        let mut vm = Vm::default();
        vm.install_web_file_api().unwrap();
        vm.install_host_console().unwrap();
        vm.install_host_error_reporting().unwrap();
        run(&mut vm, &format!("globalThis.{name}=7;")).unwrap();
        assert!(vm.install_host_rejection_tracking().is_err(), "{name}");
        assert_eq!(
            run(&mut vm, &format!("globalThis.{name}")).unwrap(),
            Value::Number(7.0)
        );
    }
}

#[test]
fn replacement_discards_pending_notifications_without_affecting_another_tab() {
    use blueice_bluejs::{
        BlueJsPageOrigin, BlueJsPageRuntime, BlueJsProgramV1, BlueJsSourceIdentity,
    };
    let origin = BlueJsPageOrigin::new("https://rejections.test").unwrap();
    let mut runtime = BlueJsPageRuntime::default();
    let configure = |runtime: &mut BlueJsPageRuntime, tab: u64| {
        runtime
            .configure_realm_bindings(tab, |host| {
                host.install_web_file_api()?;
                host.install_host_console()?;
                host.install_host_error_reporting()?;
                host.install_host_rejection_tracking()
            })
            .unwrap();
    };
    for tab in [1, 2] {
        runtime.open_realm(tab, origin.clone()).unwrap();
        configure(&mut runtime, tab);
    }
    let execute = |runtime: &mut BlueJsPageRuntime, tab: u64, serial: u64, source: &str| {
        let program = runtime
            .install_program(
                tab,
                &origin,
                BlueJsSourceIdentity::new(
                    format!("page:///rejection-{serial}.js"),
                    format!("sha256:rejection-{serial}"),
                )
                .unwrap(),
                &BlueJsProgramV1::Script(parse(source).unwrap()),
            )
            .unwrap();
        runtime.execute_program(tab, program).unwrap();
        runtime.run_microtask_checkpoint(tab).unwrap();
    };
    execute(&mut runtime,1,1,"onunhandledrejection=function(e){console.log('stale');e.preventDefault();};Promise.reject('old');");
    execute(&mut runtime, 2, 2, "Promise.reject('second');");
    runtime.navigate(1, origin.clone()).unwrap();
    configure(&mut runtime, 1);
    runtime.flush_drag_tasks(1).unwrap();
    assert!(runtime.drain_console(1).unwrap().records.is_empty());
    runtime.flush_drag_tasks(2).unwrap();
    let second = runtime.drain_console(2).unwrap();
    assert_eq!(second.records.len(), 1);
    assert!(second.records[0].message.contains("second"));
    execute(&mut runtime,1,3,"if(onunhandledrejection!==null || onrejectionhandled!==null){throw 'stale handler';}Promise.reject('fresh');");
    runtime.flush_drag_tasks(1).unwrap();
    let first = runtime.drain_console(1).unwrap();
    assert_eq!(first.records.len(), 1);
    assert!(first.records[0].message.contains("fresh"));
    assert!(runtime.close_realm(1));
    assert!(runtime.drain_console(1).is_err());
}

fn run(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
}

fn page() -> Vm {
    let mut vm = Vm::default();
    install(&mut vm);
    vm
}

fn install(vm: &mut Vm) {
    vm.install_web_file_api().unwrap();
    vm.install_window_aliases().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm.install_host_timer_functions().unwrap();
    vm.install_host_rejection_tracking().unwrap();
}

fn checkpoint(vm: &mut Vm) {
    vm.run_host_microtask_checkpoint().unwrap();
}
fn turn(vm: &mut Vm) {
    vm.run_browser_task_turn(1).unwrap();
}
fn trace(vm: &mut Vm) -> Value {
    run(vm, "trace.join(',')").unwrap()
}

#[test]
fn rejected_promises_notify_in_a_task_after_the_complete_checkpoint() {
    let mut vm = page();
    run(&mut vm,"var trace=[]; var reason={code:7}; var promise; addEventListener('error',function(){trace.push('wrong-error');}); onunhandledrejection=function(event){trace.push('unhandled'); if(event.promise!==promise || event.reason!==reason || !event.isTrusted || !event.cancelable || event.bubbles || event.target!==window || event.currentTarget!==window){throw 'wrong event';} event.preventDefault();}; promise=Promise.reject(reason); queueMicrotask(function(){trace.push('micro');}); trace.push('script');").unwrap();
    assert_eq!(trace(&mut vm), Value::String("script".into()));
    checkpoint(&mut vm);
    assert_eq!(trace(&mut vm), Value::String("script,micro".into()));
    turn(&mut vm);
    assert_eq!(
        trace(&mut vm),
        Value::String("script,micro,unhandled".into())
    );
    assert!(vm.drain_host_console().unwrap().records.is_empty());
    turn(&mut vm);
    assert_eq!(
        trace(&mut vm),
        Value::String("script,micro,unhandled".into())
    );
}

#[test]
fn handlers_before_the_checkpoint_or_before_queued_delivery_suppress_notification() {
    let mut vm = page();
    run(&mut vm,"var trace=[]; onunhandledrejection=function(e){trace.push('wrong');e.preventDefault();}; onrejectionhandled=function(){trace.push('wrong-late');}; Promise.reject(1).catch(function(){}); var early=Promise.reject(2); queueMicrotask(function(){early.catch(function(){});});").unwrap();
    checkpoint(&mut vm);
    turn(&mut vm);
    run(&mut vm, "var queued=Promise.reject(3);").unwrap();
    checkpoint(&mut vm);
    run(&mut vm, "queued.catch(function(){});").unwrap();
    checkpoint(&mut vm);
    turn(&mut vm);
    assert_eq!(trace(&mut vm), Value::String("".into()));
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn late_handlers_get_one_non_cancelable_task_after_their_reaction_job() {
    let mut vm = page();
    run(&mut vm,"var trace=[]; var p=Promise.reject('reason'); onunhandledrejection=function(e){trace.push('unhandled');e.preventDefault();}; onrejectionhandled=function(e){trace.push('handled');if(e.promise!==p || e.reason!=='reason' || e.cancelable || !e.isTrusted){throw 'wrong late event';}e.preventDefault();if(e.defaultPrevented){throw 'cancelable';}};").unwrap();
    checkpoint(&mut vm);
    turn(&mut vm);
    run(
        &mut vm,
        "p.catch(function(){trace.push('catch');}); p.catch(function(){});",
    )
    .unwrap();
    assert_eq!(trace(&mut vm), Value::String("unhandled".into()));
    checkpoint(&mut vm);
    assert_eq!(trace(&mut vm), Value::String("unhandled,catch".into()));
    turn(&mut vm);
    turn(&mut vm);
    assert_eq!(
        trace(&mut vm),
        Value::String("unhandled,catch,handled".into())
    );
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn handling_inside_unhandled_delivery_does_not_publish_a_late_handler_event() {
    let mut vm = page();
    run(&mut vm,"var trace=[]; onunhandledrejection=function(e){trace.push('unhandled'); e.promise.catch(function(){trace.push('catch');});e.preventDefault();}; onrejectionhandled=function(){trace.push('wrong');}; Promise.reject(7);").unwrap();
    checkpoint(&mut vm);
    turn(&mut vm);
    turn(&mut vm);
    assert_eq!(trace(&mut vm), Value::String("unhandled,catch".into()));
}

#[test]
fn then_adoption_and_combinators_report_only_the_unhandled_derived_promise() {
    let mut vm = page();
    run(&mut vm,"var trace=[]; var expected=[]; onunhandledrejection=function(e){trace.push(String(expected.indexOf(e.promise)));e.preventDefault();}; var first=Promise.reject('first'); expected.push(first.then()); var inner=Promise.reject('adopted'); expected.push(new Promise(function(resolve){resolve(inner);})); var input=Promise.reject('all'); expected.push(Promise.all([input]));").unwrap();
    checkpoint(&mut vm);
    turn(&mut vm);
    // The adoption's thenable job queues its reaction behind Promise.all's
    // already queued input reaction. Node v26.7.0 independently gives 0,2,1.
    assert_eq!(trace(&mut vm), Value::String("0,2,1".into()));
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn internal_awaits_consume_inputs_while_ignored_async_results_are_reported() {
    let mut vm = page();
    run(&mut vm,"var trace=[]; var result; onunhandledrejection=function(e){trace.push(e.promise===result?'async':'wrong');e.preventDefault();}; async function consume(){try{await Promise.reject('awaited');}catch(error){trace.push('caught');}} consume(); async function fail(){throw 'async';} result=fail(); async function* values(){yield Promise.reject('yielded');} values().next().catch(function(){trace.push('generator');});").unwrap();
    checkpoint(&mut vm);
    turn(&mut vm);
    assert_eq!(
        trace(&mut vm),
        Value::String("caught,generator,async".into())
    );
}

#[test]
fn uncanceled_rejections_use_bounded_diagnostics_without_a_global_error_event() {
    let mut vm = page();
    run(&mut vm,"var trace=[]; var getters=0; onerror=function(){trace.push('wrong-error');return true;}; addEventListener('unhandledrejection',function(){trace.push('listener');return true;}); Promise.reject({get message(){getters++;throw 1;},toString(){getters++;throw 2;}});").unwrap();
    checkpoint(&mut vm);
    turn(&mut vm);
    assert_eq!(trace(&mut vm), Value::String("listener".into()));
    assert_eq!(run(&mut vm, "getters").unwrap(), Value::Number(0.0));
    let batch = vm.drain_host_console().unwrap();
    assert_eq!(batch.records.len(), 1);
    assert!(batch.records[0]
        .message
        .starts_with("Uncaught (in promise) "));
    assert_eq!(batch.records[0].level, "error");
}

#[test]
fn rejection_event_construction_requires_an_object_and_keeps_opaque_identity() {
    let mut vm = page();
    assert_eq!(
        run(&mut vm, "PromiseRejectionEvent.length===2").unwrap(),
        Value::Bool(true)
    );
    assert_eq!(run(&mut vm,"var order=[];var p={};var reason={};var e=new PromiseRejectionEvent('custom',{get bubbles(){order.push('b');return false;},get cancelable(){order.push('c');return true;},get composed(){order.push('d');return false;},get promise(){order.push('p');return p;},get reason(){order.push('r');return reason;}});e instanceof Event && e instanceof PromiseRejectionEvent && e.promise===p && e.reason===reason && !e.isTrusted && order.join('')==='bcdpr' && Object.prototype.toString.call(e)==='[object PromiseRejectionEvent]'").unwrap(),Value::Bool(true));
    for expression in [
        "new PromiseRejectionEvent('x')",
        "new PromiseRejectionEvent('x',{})",
        "new PromiseRejectionEvent('x',{promise:7})",
        "PromiseRejectionEvent('x',{promise:{}})",
    ] {
        assert_eq!(
            run(
                &mut vm,
                &format!("try{{{expression};false;}}catch(error){{error instanceof TypeError;}}")
            )
            .unwrap(),
            Value::Bool(true)
        );
    }
    assert_eq!(
        run(
            &mut vm,
            "new PromiseRejectionEvent('x',{promise:p}).reason===undefined"
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn outstanding_notifications_are_weak_but_pending_delivery_survives_gc() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    config.heap.major_threshold_bytes = 1;
    let mut vm = Vm::new(config).unwrap();
    install(&mut vm);
    run(&mut vm,"var trace=[];var weak;onunhandledrejection=function(e){for(var i=0;i<100;i++){({allocated:i});}trace.push(e.reason.marker);e.preventDefault();}; (function(){var p=Promise.reject({marker:'live'});weak=new WeakRef(p);})();").unwrap();
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "weak.deref()!==undefined").unwrap(),
        Value::Bool(true)
    );
    turn(&mut vm);
    assert_eq!(trace(&mut vm), Value::String("live".into()));
    assert_eq!(
        run(&mut vm, "weak.deref()===undefined").unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn rejection_admission_and_notification_tasks_share_document_limits() {
    let mut vm = page();
    assert_eq!(
        run(
            &mut vm,
            "try{for(var i=0;i<1025;i++){Promise.reject(i);}}catch(error){} 'escaped';"
        ),
        Err(RuntimeError::SchedulingLimit)
    );
    let mut vm = page();
    run(&mut vm,"var transfer=new DataTransfer();var item=transfer.items.add('x','text/plain');for(var i=0;i<64;i++){item.getAsString(function(){});}Promise.reject('over task cap');").unwrap();
    assert_eq!(
        vm.run_host_microtask_checkpoint(),
        Err(RuntimeError::SchedulingLimit)
    );
    let mut vm = page();
    assert_eq!(run(&mut vm,"try{for(var i=0;i<12;i++){Promise.reject('x'.repeat(400000));}}catch(error){} 'escaped';"),Err(RuntimeError::SchedulingLimit));
}

#[test]
fn internal_stream_closed_promises_do_not_create_spurious_rejections() {
    let mut vm = page();
    run(&mut vm,"var trace=[];onunhandledrejection=function(e){trace.push('wrong:'+e.reason);e.preventDefault();};var reader=new ReadableStream({start(controller){controller.error('stream');}}).getReader();reader.read().catch(function(){trace.push('caught');});reader.releaseLock();").unwrap();
    checkpoint(&mut vm);
    turn(&mut vm);
    assert_eq!(trace(&mut vm), Value::String("caught".into()));
}
