// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeError, BlueJsProgramV1,
    BlueJsSourceIdentity, RuntimeError, Value, Vm, MAX_HOST_SCHEDULING_BYTES,
};

fn page() -> BlueJsPageRuntime {
    let mut runtime = BlueJsPageRuntime::default();
    runtime
        .open_realm(
            1,
            BlueJsPageOrigin::new("https://agent-registry.test").unwrap(),
        )
        .unwrap();
    runtime
}

fn execute(
    runtime: &mut BlueJsPageRuntime,
    tab: u64,
    source: &str,
) -> Result<Value, BlueJsPageRuntimeError> {
    let stats = runtime.realm_stats(tab).unwrap();
    let program = runtime
        .install_program(
            tab,
            &stats.origin,
            BlueJsSourceIdentity::new(
                format!("page:///registry-{}.js", stats.program_count),
                "registry",
            )
            .unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    runtime.execute_program(tab, program)
}

#[test]
fn registry_payload_cannot_bypass_the_terminal_document_native_budget() {
    let mut runtime = page();
    let origin = BlueJsPageOrigin::new("https://agent-registry.test").unwrap();
    runtime.open_realm(2, origin.clone()).unwrap();
    let result = execute(
        &mut runtime,
        1,
        "for(var i=0;i<16;i++){Symbol.for('x'.repeat(400000)+String(i));}0;",
    );
    assert!(
        matches!(
            result,
            Err(BlueJsPageRuntimeError::Runtime(
                RuntimeError::SchedulingLimit
            ))
        ),
        "registry bypassed native admission: {result:?}"
    );
    assert!(runtime.realm_stats(1).unwrap().scheduling.payload_bytes <= MAX_HOST_SCHEDULING_BYTES);
    assert!(execute(&mut runtime, 1, "42;").is_err());
    assert!(runtime.run_microtask_checkpoint(1).is_err());
    assert_eq!(
        execute(&mut runtime, 2, "42;").unwrap(),
        Value::Number(42.0)
    );
    runtime.navigate(1, origin).unwrap();
    assert_eq!(runtime.realm_stats(1).unwrap().scheduling.payload_bytes, 0);
    assert_eq!(
        execute(&mut runtime, 1, "42;").unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn completed_jobs_report_agent_storage_and_repeated_keys_do_not_recharge_it() {
    // Discard setup completions: active Symbol descriptions have their own
    // execution charge, independently of the registry storage under test.
    let mut runtime = page();
    execute(
        &mut runtime,
        1,
        "for(var i=0;i<3;i++){Symbol.for('x'.repeat(400000)+String(i));}0;",
    )
    .unwrap();
    let bytes = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    assert!(
        bytes >= 4800000,
        "registry storage omitted after the job: {bytes}"
    );
    let before = runtime.realm_stats(1).unwrap().heap.major_collections;
    assert_eq!(
        execute(
            &mut runtime,
            1,
            "Symbol.for('x'.repeat(400000)+'0')===Symbol.for('x'.repeat(400000)+'0');"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        runtime.realm_stats(1).unwrap().scheduling.payload_bytes,
        bytes
    );
    assert!(runtime.realm_stats(1).unwrap().heap.major_collections > before);
}

#[test]
fn shadow_realms_share_one_charge_and_child_reclamation_preserves_registered_identity() {
    let mut runtime = page();
    execute(
        &mut runtime,
        1,
        "var remembered=Symbol.for('x'.repeat(400000));0;",
    )
    .unwrap();
    let before = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    execute(
        &mut runtime,
        1,
        "var r=new ShadowRealm();r.evaluate(\"Symbol.for('x'.repeat(400000));0;\");0;",
    )
    .unwrap();
    assert_eq!(
        runtime.realm_stats(1).unwrap().scheduling.payload_bytes,
        before
    );
    execute(
        &mut runtime,
        1,
        "var childSymbol=r.evaluate(\"Symbol.for('y'.repeat(400000));\");0;",
    )
    .unwrap();
    let after = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    assert!(after >= before + 1600000);
    assert_eq!(execute(&mut runtime,1,"r=null;childSymbol===Symbol.for('y'.repeat(400000)) && remembered===Symbol.for('x'.repeat(400000));").unwrap(),Value::Bool(true));
    assert_eq!(
        runtime.realm_stats(1).unwrap().scheduling.payload_bytes,
        after
    );
}

#[test]
fn nested_realms_cannot_split_the_agent_registry_budget() {
    let mut runtime = page();
    execute(
        &mut runtime,
        1,
        "for(var i=0;i<3;i++)void Symbol.for('x'.repeat(400000)+String(i));0;",
    )
    .unwrap();
    let source = "for(var i=0;i<3;i++)Symbol.for('y'.repeat(400000)+String(i));0;";
    let nested = format!("new ShadowRealm().evaluate({source:?});");
    let result = execute(
        &mut runtime,
        1,
        &format!("new ShadowRealm().evaluate({nested:?});"),
    );
    assert!(
        matches!(
            result,
            Err(BlueJsPageRuntimeError::Runtime(
                RuntimeError::SchedulingLimit
            ))
        ),
        "{result:?}"
    );
    assert!(runtime.realm_stats(1).unwrap().scheduling.payload_bytes <= MAX_HOST_SCHEDULING_BYTES);
    assert!(runtime.run_microtask_checkpoint(1).is_err());
}

#[test]
fn registry_and_existing_promise_or_timer_storage_use_one_cap() {
    for producer in ["timer", "promise"] {
        let mut runtime = page();
        runtime
            .configure_realm_bindings(1, |host| host.install_host_timers())
            .unwrap();
        execute(&mut runtime,1,"for(var i=0;i<4;i++)void Symbol.for('x'.repeat(400000)+String(i));var held=[];function noop(){}0;").unwrap();
        let source = if producer == "timer" {
            "for(var i=0;i<3;i++)setTimeout(noop,10000,'y'.repeat(400000));0;"
        } else {
            "for(var i=0;i<3;i++)held.push(Promise.resolve('y'.repeat(400000)));0;"
        };
        let result = execute(&mut runtime, 1, source);
        assert!(
            matches!(
                result,
                Err(BlueJsPageRuntimeError::Runtime(
                    RuntimeError::SchedulingLimit
                ))
            ),
            "{producer}: {result:?}"
        );
        assert!(
            runtime.realm_stats(1).unwrap().scheduling.payload_bytes <= MAX_HOST_SCHEDULING_BYTES
        );
        assert!(runtime.run_task_turn(1, 10000, 64).is_err());
    }
}

#[test]
fn refused_registry_growth_does_not_increase_committed_native_bytes() {
    let mut runtime = page();
    execute(&mut runtime,1,"for(var i=0;i<3;i++)void Symbol.for('x'.repeat(400000)+String(i));var held=[];for(var i=0;i<3;i++)held.push(Promise.resolve('y'.repeat(350000)));0;").unwrap();
    let before = runtime.realm_stats(1).unwrap().scheduling.payload_bytes;
    let result = execute(&mut runtime, 1, "Symbol.for('new'+'x'.repeat(400000));0;");
    assert!(
        matches!(
            result,
            Err(BlueJsPageRuntimeError::Runtime(
                RuntimeError::SchedulingLimit
            ))
        ),
        "{result:?}"
    );
    assert_eq!(
        runtime.realm_stats(1).unwrap().scheduling.payload_bytes,
        before
    );
}

#[test]
fn table_growth_utf16_keys_and_reentrant_coercion_preserve_symbol_semantics() {
    let mut runtime = page();
    assert_eq!(execute(&mut runtime,1,"var first=Symbol.for('first');for(var i=0;i<128;i++)Symbol.for('small'+String(i));var conversions=0;var key={toString(){conversions++;Symbol.for('inner');return 'outer';}};var outer=Symbol.for(key);var lone=Symbol.for('\\uD800');first===Symbol.for('first') && outer===Symbol.for('outer') && conversions===1 && Symbol.keyFor(lone)==='\\uD800' && lone!==Symbol.for('\\uFFFD');").unwrap(),Value::Bool(true));
    assert_eq!(execute(&mut runtime,1,"var refused=false;try{new WeakRef(first);}catch(error){refused=error instanceof TypeError;}refused;").unwrap(),Value::Bool(true));
}

fn script(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
}

fn error_bindings(vm: &mut Vm) {
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_host_error_reporting().unwrap();
}

#[test]
fn host_neutral_registry_is_unbounded_and_oversized_policy_attachment_is_atomic() {
    let mut vm = Vm::default();
    script(
        &mut vm,
        "for(var i=0;i<16;i++)void Symbol.for('x'.repeat(400000)+String(i));0;",
    )
    .unwrap();
    error_bindings(&mut vm);
    assert_eq!(
        vm.install_host_rejection_tracking(),
        Err(RuntimeError::SchedulingLimit)
    );
    assert_eq!(script(&mut vm,"typeof PromiseRejectionEvent==='undefined' && typeof onunhandledrejection==='undefined' && Symbol.for('x'.repeat(400000)+'0')===Symbol.for('x'.repeat(400000)+'0');").unwrap(),Value::Bool(true));
}

#[test]
fn enabling_document_policy_charges_existing_agent_entries() {
    let mut vm = Vm::default();
    script(&mut vm, "Symbol.for('x'.repeat(400000));0;").unwrap();
    error_bindings(&mut vm);
    vm.install_host_rejection_tracking().unwrap();
    assert!(vm.host_scheduling_usage().unwrap().payload_bytes >= 1600000);
    assert_eq!(
        script(
            &mut vm,
            "Symbol.keyFor(Symbol.for('x'.repeat(400000))).length;"
        )
        .unwrap(),
        Value::Number(400000.0)
    );
}
