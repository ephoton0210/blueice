// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsProgramV1, BlueJsSourceIdentity,
    Value, Vm, MAX_HOST_CONSOLE_BYTES, MAX_HOST_CONSOLE_MESSAGE_BYTES, MAX_HOST_CONSOLE_RECORDS,
};

#[test]
fn draining_console_does_not_run_queued_jobs_or_retain_their_objects() {
    let mut vm = page();
    run(
        &mut vm,
        "Promise.resolve().then(function(){console.log('job');}); console.log('turn');",
    );
    assert_eq!(vm.drain_host_console().unwrap().records[0].message, "turn");
    assert!(vm.drain_host_console().unwrap().records.is_empty());
    vm.run_promise_jobs().unwrap();
    assert_eq!(vm.drain_host_console().unwrap().records[0].message, "job");
}

#[test]
fn console_buffers_and_label_state_follow_tabs_navigation_and_close() {
    let origin = BlueJsPageOrigin::new("https://console.test").unwrap();
    let mut runtime = BlueJsPageRuntime::default();
    for tab in [1, 2] {
        runtime.open_realm(tab, origin.clone()).unwrap();
        runtime
            .configure_realm_bindings(tab, |host| host.install_host_console())
            .unwrap();
    }
    let execute = |runtime: &mut BlueJsPageRuntime, tab, source: &str| {
        let program = runtime
            .install_program(
                tab,
                &origin,
                BlueJsSourceIdentity::new("page:///console.js", format!("sha256:{source}"))
                    .unwrap(),
                &BlueJsProgramV1::Script(parse(source).unwrap()),
            )
            .unwrap();
        runtime.execute_program(tab, program).unwrap();
    };
    execute(
        &mut runtime,
        1,
        "console.count('first'); console.log('undrained');",
    );
    execute(&mut runtime, 2, "console.error('second');");
    assert_eq!(
        runtime.drain_console(2).unwrap().records[0].message,
        "second"
    );
    runtime.navigate(1, origin.clone()).unwrap();
    runtime
        .configure_realm_bindings(1, |host| host.install_host_console())
        .unwrap();
    assert!(runtime.drain_console(1).unwrap().records.is_empty());
    execute(&mut runtime, 1, "console.count('first');");
    assert_eq!(
        runtime.drain_console(1).unwrap().records[0].message,
        "first: 1"
    );
    assert!(runtime.close_realm(1));
    assert!(runtime.drain_console(1).is_err());
    assert!(runtime.drain_console(2).unwrap().records.is_empty());
}
fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}
fn page() -> Vm {
    let mut vm = Vm::default();
    vm.install_host_console().unwrap();
    vm
}

#[test]
fn long_unknown_format_specifiers_obey_native_instruction_fuel() {
    let mut vm = Vm::new(blueice_bluejs::VmConfig {
        instruction_budget: 100,
        ..Default::default()
    })
    .unwrap();
    vm.install_host_console().unwrap();
    let source = format!("console.log('x{}', 1);", "%x".repeat(10000));
    let result = vm.execute_script(&compile(&parse(&source).unwrap()).unwrap());
    assert!(matches!(
        result,
        Err(blueice_bluejs::RuntimeError::InstructionLimit)
    ));
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn count_labels_preserve_utf16_identity_and_state_caps_do_not_stop_the_script() {
    let mut vm = page();
    run(
        &mut vm,
        "console.count('\\ud800'); console.count('�'); console.count('\\ud800');",
    );
    let records = vm.drain_host_console().unwrap().records;
    assert!(records[0].message.ends_with(": 1"));
    assert!(records[1].message.ends_with(": 1"));
    assert!(records[2].message.ends_with(": 2"));
    run(&mut vm,"for(var i=0;i<140;i++){console.count('label'+i);console.time('timer'+i);} console.count('x'.repeat(300)); var survived=true;");
    assert_eq!(run(&mut vm, "survived"), Value::Bool(true));
    let records = vm.drain_host_console().unwrap().records;
    assert!(records
        .iter()
        .any(|record| record.message.contains("label limit")));
    assert!(records
        .iter()
        .any(|record| record.message.contains("byte limit")));
}

#[test]
fn console_is_owner_selected_and_namespace_methods_are_callable_without_a_receiver() {
    let mut vm = Vm::default();
    assert_eq!(
        run(&mut vm, "typeof console"),
        Value::String("undefined".into())
    );
    vm.install_host_console().unwrap();
    assert_eq!(run(&mut vm, "Object.keys(console).length === 19 && Object.getOwnPropertyNames(Object.getPrototypeOf(console)).length === 0 && Object.getPrototypeOf(Object.getPrototypeOf(console)) === Object.prototype"), Value::Bool(true));
    assert_eq!(run(&mut vm, "var log = console.log; log('hello', 7, true, null, undefined, 9n, Symbol('s')); console.log.length === 0 && console.log.name === 'log'"), Value::Bool(true));
    let batch = vm.drain_host_console().unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(
        batch.records[0].message,
        "hello 7 true null undefined 9n Symbol(s)"
    );
    assert!(vm.drain_host_console().unwrap().records.is_empty());
    assert!(vm.install_host_console().is_err());
    assert_eq!(
        run(
            &mut vm,
            "try { new console.log(); } catch (error) { error instanceof TypeError; }"
        ),
        Value::Bool(true)
    );
}

#[test]
fn formatting_coerces_substitutions_and_previews_objects_without_getters() {
    let mut vm = page();
    run(&mut vm,"console.log('%s %d %i %f %o %O %cend %%', Symbol('s'), '12px', '4.9', '2.5px', {x: 1}, [2, 'a'], 'color:red'); var calls=0; console.log({get secret(){ calls++; throw 1; }}); console.log(new Proxy({}, {ownKeys(){throw 2;}}));");
    let batch = vm.drain_host_console().unwrap();
    assert_eq!(
        batch.records[0].message,
        "Symbol(s) 12 4 2.5 {x: 1} [2, \"a\"] end %%"
    );
    assert_eq!(run(&mut vm, "calls"), Value::Number(0.0));
    assert_eq!(batch.records.len(), 3);
    run(
        &mut vm,
        "console.log('%s %d', {toString(){ console.info('inner'); return 'outer'; }}, Symbol());",
    );
    let records = vm.drain_host_console().unwrap().records;
    assert_eq!(
        records
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        ["inner", "outer NaN"]
    );
}

#[test]
fn conversions_propagate_page_exceptions_without_fabricating_a_log() {
    let mut vm = page();
    assert_eq!(run(&mut vm,"var caught=false; try { console.log('%s', {toString(){throw 'fail';}}); } catch (error) {caught=error==='fail';} caught"),Value::Bool(true));
    assert!(vm.drain_host_console().unwrap().records.is_empty());
    assert_eq!(
        run(
            &mut vm,
            "try { console.count(Symbol()); } catch (error) { error instanceof TypeError; }"
        ),
        Value::Bool(true)
    );
}

#[test]
fn assertions_counts_groups_clear_and_trace_keep_their_own_state() {
    let mut vm = page();
    run(&mut vm,"console.assert(true, 'quiet'); console.assert(false, 'bad %d', 3); console.count(); console.count(); console.countReset(); console.count(); console.group('g'); console.groupCollapsed('h'); console.warn('nested'); console.groupEnd(); console.groupEnd(); function traceMe(){console.trace('trace');} traceMe();");
    let records = vm.drain_host_console().unwrap().records;
    assert_eq!(records[0].message, "Assertion failed: bad 3");
    assert_eq!(records[1].message, "default: 1");
    assert_eq!(records[2].message, "default: 2");
    assert_eq!(records[3].message, "default: 1");
    assert_eq!(records[6].group_depth, 2);
    assert_eq!(records[6].level, "warn");
    assert!(records.last().unwrap().message.contains("traceMe"));
    run(
        &mut vm,
        "console.log('discard'); console.clear(); console.count();",
    );
    let records = vm.drain_host_console().unwrap().records;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].method, "clear");
    assert_eq!(records[1].message, "default: 2");
}

#[test]
fn timers_use_the_owner_clock_and_missing_or_duplicate_labels_warn() {
    let mut vm = page();
    run(&mut vm, "console.time('load'); console.time('load');");
    vm.run_due_host_timers(25).unwrap();
    run(
        &mut vm,
        "console.timeLog('load', 'stage'); console.timeEnd('load'); console.timeEnd('load');",
    );
    let records = vm.drain_host_console().unwrap().records;
    assert_eq!(records[0].level, "warn");
    assert_eq!(records[1].message, "load: 25 ms stage");
    assert_eq!(records[2].message, "load: 25 ms");
    assert_eq!(records[3].level, "warn");
}

#[test]
fn console_storage_and_utf8_messages_are_bounded_and_accounted() {
    let mut vm = page();
    // A first identical run leaves behind only what the script itself retains
    // (its global binding and program), which is not console storage.
    run(&mut vm, "for(var i=0;i<300;i++){console.log(i);}");
    vm.drain_host_console().unwrap();
    let base = vm.host_scheduling_usage().unwrap().payload_bytes;
    run(&mut vm, "for(var i=0;i<300;i++){console.log(i);}");
    assert!(vm.host_scheduling_usage().unwrap().payload_bytes > base);
    let batch = vm.drain_host_console().unwrap();
    assert_eq!(batch.records.len(), MAX_HOST_CONSOLE_RECORDS);
    assert_eq!(batch.dropped_records, 44);
    assert_eq!(batch.records[0].message, "44");
    assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes, base);
    run(
        &mut vm,
        "for(var i=0;i<100;i++){console.log('🐧'.repeat(4000));}",
    );
    let batch = vm.drain_host_console().unwrap();
    assert!(batch
        .records
        .iter()
        .all(|record| record.truncated && record.message.len() <= MAX_HOST_CONSOLE_MESSAGE_BYTES));
    assert!(
        batch
            .records
            .iter()
            .map(|record| record.message.capacity())
            .sum::<usize>()
            <= MAX_HOST_CONSOLE_BYTES
    );
    assert!(batch.dropped_records > 0);
    run(
        &mut vm,
        "console.log('x'.repeat(8191) + '🐧', 'tail'); console.log({get x(){throw 1;}}, 'after');",
    );
    let records = vm.drain_host_console().unwrap().records;
    assert_eq!(records[0].message, "x".repeat(8191));
    assert!(records[0].truncated);
    assert_eq!(records[1].message, "[Object] after");
}
