// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, compile_module, parse, parse_module, RuntimeError, Value, Vm, VmConfig,
};
use std::collections::HashMap;

fn run(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
}

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_window_aliases().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm
}

#[test]
fn error_reporting_is_owner_selected_and_refuses_existing_names() {
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    assert_eq!(
        run(
            &mut vm,
            "typeof reportError + ',' + typeof ErrorEvent + ',' + typeof onerror"
        )
        .unwrap(),
        Value::String("undefined,undefined,undefined".into())
    );
    assert!(vm.install_host_error_reporting().is_ok());
    assert!(vm.install_host_error_reporting().is_err());
    assert_eq!(run(&mut vm, "onerror === null && reportError.length === 1 && addEventListener.length === 2 && removeEventListener.length === 2 && dispatchEvent.length === 1 && globalThis instanceof EventTarget").unwrap(), Value::Bool(true));
    assert_eq!(run(&mut vm, "['reportError','addEventListener','removeEventListener','dispatchEvent'].every(function(name){var property=Object.getOwnPropertyDescriptor(globalThis,name); return property.writable && property.enumerable && property.configurable;})").unwrap(), Value::Bool(true));
    for name in [
        "reportError",
        "ErrorEvent",
        "onerror",
        "addEventListener",
        "removeEventListener",
        "dispatchEvent",
    ] {
        let mut vm = Vm::default();
        vm.install_web_file_api().unwrap();
        vm.install_host_console().unwrap();
        run(&mut vm, &format!("globalThis.{name} = 7;")).unwrap();
        assert!(vm.install_host_error_reporting().is_err(), "{name}");
        assert_eq!(
            run(&mut vm, &format!("globalThis.{name}")).unwrap(),
            Value::Number(7.0)
        );
    }
}

#[test]
fn error_events_preserve_brand_values_and_dictionary_conversion_order() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var order=[]; var original={}; var event=new ErrorEvent('problem', {get bubbles(){order.push('b');return true;},get cancelable(){order.push('c');return true;},get composed(){order.push('d');return false;},get colno(){order.push('e');return -1;},get error(){order.push('f');return original;},get filename(){order.push('g');return '\\ud800';},get lineno(){order.push('h');return 4294967297;},get message(){order.push('i');return 'message';}}); event instanceof ErrorEvent && event instanceof Event && event.error === original && event.message === 'message' && event.filename === '�' && event.colno === 4294967295 && event.lineno === 1 && !event.isTrusted && event.bubbles && event.cancelable && order.join('') === 'bcdefghi'").unwrap(), Value::Bool(true));
    assert_eq!(run(&mut vm, "var defaults=new ErrorEvent('error'); defaults.message === '' && defaults.filename === '' && defaults.lineno === 0 && defaults.colno === 0 && defaults.error === undefined && !defaults.cancelable && Object.prototype.toString.call(defaults) === '[object ErrorEvent]'").unwrap(), Value::Bool(true));
    assert_eq!(run(&mut vm, "var getter=Object.getOwnPropertyDescriptor(ErrorEvent.prototype,'error').get; try {getter.call(new Event('x'));} catch(error) {error instanceof TypeError;}").unwrap(), Value::Bool(true));
}

#[test]
fn reporting_delivers_original_values_and_onerror_true_cancels_in_listener_order() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var original=new Error('probe'); var log=[]; addEventListener('error',function(e){log.push('before'); if(e.error !== original || e.target !== window || e.currentTarget !== window || !e.isTrusted || e.bubbles || !e.cancelable){throw 'wrong event';}}); window.onerror=function(message,filename,line,column,error){log.push('handler'); return arguments.length===5 && this===window && error===original && typeof message==='string' && filename==='' && line===0 && column===0;}; addEventListener('error',function(e){log.push('after:'+e.defaultPrevented);}); reportError(original); log.join(',')").unwrap(), Value::String("before,handler,after:true".into()));
    assert!(vm.drain_host_console().unwrap().records.is_empty());
    run(
        &mut vm,
        "onerror=function(){return false;}; reportError(original);",
    )
    .unwrap();
    let messages = vm.drain_host_console().unwrap().records;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].level, "error");
    assert!(messages[0].message.contains("probe"));
}

#[test]
fn cancellation_listener_options_and_handler_replacement_keep_existing_semantics() {
    let mut vm = page();
    assert_eq!(run(&mut vm, "var log=[]; var controller=new AbortController(); addEventListener('error',function(){log.push('aborted');},{signal:controller.signal}); controller.abort(); addEventListener('error',function(e){log.push('once'); e.preventDefault();},{once:true}); onerror=function(){log.push('old');}; addEventListener('error',function(){log.push('last');}); onerror=function(){log.push('new'); return true;}; reportError(7); reportError(8); log.join(',')").unwrap(), Value::String("once,new,last,new,last".into()));
    assert!(vm.drain_host_console().unwrap().records.is_empty());
    assert_eq!(run(&mut vm, "onerror=null; var event=new ErrorEvent('error',{cancelable:true,error:42}); onerror=function(a,b,c,d,e){return e===42;}; !window.dispatchEvent(event) && event.defaultPrevented && !event.isTrusted").unwrap(), Value::Bool(true));
}

#[test]
fn recursive_reports_and_handler_throws_do_not_recursively_dispatch_error_events() {
    let mut vm = page();
    run(&mut vm, "var calls=0; onerror=function(){calls++; reportError('nested'); throw new Error('handler');}; reportError('outer');").unwrap();
    assert_eq!(run(&mut vm, "calls").unwrap(), Value::Number(1.0));
    let messages = vm.drain_host_console().unwrap().records;
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].message.contains("nested"));
    assert!(messages[1].message.contains("handler"));
    assert!(messages[2].message.contains("outer"));
}

#[test]
fn timer_frame_microtask_and_event_exceptions_report_once_and_continue_peers() {
    let mut vm = page();
    vm.install_host_timer_functions().unwrap();
    vm.install_host_animation_frame_functions().unwrap();
    run(&mut vm, "var log=[]; onerror=function(a,b,c,d,e){log.push('error:'+e); return true;}; setTimeout(function(){throw 'timer';},0); setTimeout(function(){log.push('peer');},0); queueMicrotask(function(){throw 'microtask';}); requestAnimationFrame(function(){throw 'frame';}); var target=new EventTarget(); target.addEventListener('x',function(){throw 'event';}); target.addEventListener('x',function(){log.push('listener');}); target.dispatchEvent(new Event('x')); Promise.reject('rejection');").unwrap();
    let timers = vm.run_due_host_timers(0).unwrap();
    assert_eq!(timers.tasks_run, 2);
    assert_eq!(timers.uncaught_errors, 2);
    assert_eq!(vm.run_host_animation_frame(1.0).unwrap().uncaught_errors, 1);
    assert_eq!(
        run(&mut vm, "log.join(',')").unwrap(),
        Value::String("error:event,listener,error:timer,error:microtask,peer,error:frame".into())
    );
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn uncaught_root_errors_report_without_changing_the_public_execution_error() {
    let mut vm = page();
    run(
        &mut vm,
        "var error=new Error('root'); var seen; onerror=function(a,b,c,d,e){seen=e; return true;};",
    )
    .unwrap();
    assert!(matches!(
        run(&mut vm, "throw error;"),
        Err(RuntimeError::Thrown(_))
    ));
    assert_eq!(run(&mut vm, "seen===error").unwrap(), Value::Bool(true));
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn reporting_does_not_run_error_value_getters_and_does_not_retain_live_values() {
    let mut vm = page();
    run(&mut vm, "var calls=0; var weak; (function(){var value={get message(){calls++; throw 1;},toString(){calls++;throw 2;}}; weak=new WeakRef(value); reportError(value);})();").unwrap();
    assert_eq!(
        run(&mut vm, "calls===0 && weak.deref()===undefined").unwrap(),
        Value::Bool(true)
    );
    assert_eq!(vm.drain_host_console().unwrap().records.len(), 1);
}

#[test]
fn error_event_values_survive_collection_during_dispatch_and_die_after_release() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    config.heap.major_threshold_bytes = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm.install_host_timer_functions().unwrap();
    run(&mut vm, "var saved; var weak; addEventListener('error',function(event){saved=event; for(var i=0;i<100;i++){({allocation:i});} event.preventDefault();}); setTimeout(function(){var value={marker:'timer'}; weak=new WeakRef(value); throw value;},0);").unwrap();
    assert_eq!(vm.run_due_host_timers(0).unwrap().uncaught_errors, 1);
    assert_eq!(
        run(
            &mut vm,
            "saved.error.marker==='timer' && weak.deref()===saved.error"
        )
        .unwrap(),
        Value::Bool(true)
    );
    run(&mut vm, "saved=null;").unwrap();
    assert_eq!(
        run(&mut vm, "weak.deref()===undefined").unwrap(),
        Value::Bool(true)
    );
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn runaway_error_handlers_obey_the_existing_instruction_limit() {
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 5000,
        ..Default::default()
    })
    .unwrap();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
    assert_eq!(
        run(
            &mut vm,
            "onerror=function(){while(true){}}; try {reportError(1);} catch(error) {42;}"
        ),
        Err(RuntimeError::InstructionLimit)
    );
}

#[test]
fn asynchronous_drag_string_exceptions_report_before_the_task_checkpoint() {
    let mut vm = page();
    vm.install_host_timer_functions().unwrap();
    run(&mut vm, "var log=[]; onerror=function(a,b,c,d,error){log.push(error.marker);return true;}; var transfer=new DataTransfer(); transfer.items.add('drag','text/plain').getAsString(function(text){queueMicrotask(function(){log.push('micro');});throw {marker:text};});").unwrap();
    vm.run_browser_tasks_bounded(1).unwrap();
    assert_eq!(
        run(&mut vm, "log.join(',')").unwrap(),
        Value::String("drag,micro".into())
    );
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn page_module_failures_report_once_but_caught_dynamic_imports_do_not_report() {
    let mut vm = page();
    vm.install_host_timer_functions().unwrap();
    run(
        &mut vm,
        "var seen=[]; onerror=function(a,b,c,d,e){seen.push(e.message);return true;};",
    )
    .unwrap();
    let modules = HashMap::from([
        ("sync.js".to_string(), compile_module(&parse_module("throw new Error('sync');").unwrap()).unwrap()),
        ("async.js".to_string(), compile_module(&parse_module("await new Promise(function(resolve){setTimeout(resolve,0);}); throw new Error('async');").unwrap()).unwrap()),
        ("imported.js".to_string(), compile_module(&parse_module("throw new Error('imported');").unwrap()).unwrap()),
    ]);
    assert!(vm.execute_module_graph("sync.js", &modules).is_err());
    vm.execute_module_graph("async.js", &modules).unwrap();
    vm.run_due_host_timers(0).unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    vm.set_module_loader_context("main.js", modules);
    run(
        &mut vm,
        "import('./imported.js').catch(function(error){seen.push('caught:'+error.message);});",
    )
    .unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(
        run(&mut vm, "seen.join(',')").unwrap(),
        Value::String("sync,async,caught:imported".into())
    );
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn asynchronous_dependency_parent_failure_settles_the_graph_without_stalling_siblings() {
    let mut vm = page();
    vm.install_host_timer_functions().unwrap();
    let sources = [
        ("main.js", "import './bad.js';import './good.js';"),
        ("dep.js", "await 0;"),
        ("bad.js", "import './dep.js';throw 'parent-failure';"),
        ("good.js", "import './dep.js';globalThis.siblingFinished=1;"),
    ];
    let modules = sources
        .into_iter()
        .map(|(name, source)| {
            (
                name.to_owned(),
                compile_module(&parse_module(source).unwrap()).unwrap(),
            )
        })
        .collect();
    assert!(vm.execute_module_graph("main.js", &modules).is_err());
    vm.run_host_microtask_checkpoint().unwrap();
    assert_eq!(run(&mut vm, "siblingFinished").unwrap(), Value::Number(1.0));
    let messages = vm.drain_host_console().unwrap();
    assert_eq!(messages.records.len(), 1, "{messages:?}");
    assert!(messages.records[0].message.contains("parent-failure"));
    vm.run_host_microtask_checkpoint().unwrap();
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn asynchronous_dependency_parent_limits_keep_language_failures_and_host_aborts_distinct() {
    for (body, language) in [("'x'.repeat(4096);", true), ("while(true){}", false)] {
        let config = VmConfig {
            max_string_bytes: 2048,
            instruction_budget: 512,
            ..VmConfig::default()
        };
        let mut vm = Vm::new(config).unwrap();
        vm.install_web_file_api().unwrap();
        vm.install_host_console().unwrap();
        vm.install_window_aliases().unwrap();
        vm.install_host_error_reporting().unwrap();
        let entry = format!("import './dep.js';{body}");
        let modules = [
            (
                "main.js".to_owned(),
                compile_module(&parse_module(&entry).unwrap()).unwrap(),
            ),
            (
                "dep.js".to_owned(),
                compile_module(&parse_module("await 0;").unwrap()).unwrap(),
            ),
        ]
        .into_iter()
        .collect();
        let error = vm.execute_module_graph("main.js", &modules).unwrap_err();
        let messages = vm.drain_host_console().unwrap();
        if language {
            assert!(matches!(error, RuntimeError::Thrown(_)), "{error:?}");
            assert_eq!(messages.records.len(), 1, "{messages:?}");
        } else {
            assert_eq!(error, RuntimeError::InstructionLimit);
            assert!(messages.records.is_empty());
        }
    }
}
