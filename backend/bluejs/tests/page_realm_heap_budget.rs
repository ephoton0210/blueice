// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig, BlueJsProgramV1,
    BlueJsSourceIdentity, HeapError, RuntimeError, Value,
};

fn execute(
    runtime: &mut BlueJsPageRuntime,
    tab: u64,
    source: &str,
) -> Result<Value, blueice_bluejs::BlueJsPageRuntimeError> {
    let stats = runtime.realm_stats(tab).unwrap();
    let program = runtime
        .install_program(
            tab,
            &stats.origin,
            BlueJsSourceIdentity::new(
                format!("page:///heap-{}.js", stats.program_count),
                "heap-budget",
            )
            .unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    runtime.execute_program(tab, program)
}

#[test]
fn live_child_heaps_cannot_multiply_the_document_heap_budget() {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.heap.max_heap_bytes = 1024 * 1024;
    let mut runtime = BlueJsPageRuntime::new(config).unwrap();
    let origin = BlueJsPageOrigin::new("https://realm-heap.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    runtime.open_realm(2, origin.clone()).unwrap();
    let result = execute(&mut runtime, 1, "var realms=[];for(var i=0;i<16;i++){var r=new ShadowRealm();r.evaluate(\"globalThis.retained='x'.repeat(400000);0;\");realms.push(r);}realms.length;");
    assert!(
        matches!(
            result,
            Err(blueice_bluejs::BlueJsPageRuntimeError::Runtime(
                RuntimeError::Heap(HeapError::HeapLimitExceeded { limit: 1048576 })
            ))
        ),
        "sixteen child heaps bypassed the document cap: {result:?}"
    );
    assert!(
        execute(&mut runtime, 1, "42;").is_err(),
        "document heap exhaustion must stay terminal"
    );
    assert_eq!(
        execute(&mut runtime, 2, "42;").unwrap(),
        Value::Number(42.0)
    );
    runtime.navigate(1, origin).unwrap();
    assert_eq!(
        execute(&mut runtime, 1, "42;").unwrap(),
        Value::Number(42.0)
    );
}

fn page(limit: usize) -> BlueJsPageRuntime {
    let mut config = BlueJsPageRuntimeConfig::default();
    config.vm.heap.max_heap_bytes = limit;
    config.vm.max_string_bytes = limit;
    let mut runtime = BlueJsPageRuntime::new(config).unwrap();
    runtime
        .open_realm(1, BlueJsPageOrigin::new("https://realm-heap.test").unwrap())
        .unwrap();
    runtime
}

#[test]
fn wrapped_owners_keep_their_heap_charge_and_last_owner_release_removes_it() {
    let mut runtime = page(2 * 1024 * 1024);
    execute(
        &mut runtime,
        1,
        "var read=new ShadowRealm().evaluate(\"var held='x'.repeat(300000);()=>held.length;\");",
    )
    .unwrap();
    assert_eq!(
        execute(&mut runtime, 1, "read();").unwrap(),
        Value::Number(300000.0)
    );
    let stats = runtime.realm_stats(1).unwrap();
    assert!(stats.document_heap_bytes >= stats.heap.managed_bytes + 600000);
    execute(&mut runtime, 1, "read=null;0;").unwrap();
    let stats = runtime.realm_stats(1).unwrap();
    assert_eq!(stats.document_heap_bytes, stats.heap.managed_bytes);
}

#[test]
fn discarded_managed_realm_payloads_do_not_exhaust_timer_turns() {
    for document_turn in [false, true] {
        let mut runtime = page(2 * 1024 * 1024);
        runtime
            .configure_realm_bindings(1, |host| host.install_host_timers())
            .unwrap();
        execute(&mut runtime, 1, "function poll(){new ShadowRealm().evaluate(\"var held='x'.repeat(300000);0;\");}setInterval(poll,10);").unwrap();
        for tick in 1..=64 {
            let turn = if document_turn {
                runtime.run_task_turn(1, tick * 10, 1)
            } else {
                runtime
                    .run_due_timers(1, tick * 10)
                    .map(|turn| blueice_bluejs::BlueJsTaskTurn {
                        tasks_run: turn.tasks_run,
                        next_wake_in_ms: None,
                    })
            };
            assert_eq!(
                turn.unwrap_or_else(|e| panic!("turn={document_turn}, tick={tick}: {e:?}"))
                    .tasks_run,
                1
            );
            assert!(runtime.realm_stats(1).unwrap().document_heap_bytes <= 2 * 1024 * 1024);
        }
    }
}

#[test]
fn property_collection_buffer_allocation_and_resize_share_the_document_envelope() {
    for (name, setup, growth) in [
        (
            "property",
            "globalThis.held='x'.repeat(300000);",
            "globalThis.extra='y'.repeat(600000);",
        ),
        (
            "buffer allocation",
            "globalThis.held='x'.repeat(300000);",
            "globalThis.extra=new ArrayBuffer(1200000);",
        ),
        (
            "buffer resize",
            "var buffer=new ArrayBuffer(600000,{maxByteLength:1900000});",
            "buffer.resize(1800000);",
        ),
        (
            "map",
            "globalThis.held='x'.repeat(300000);var items=new Map();",
            "for(var i=0;i<6;i++)items.set(i,'y'.repeat(100000));",
        ),
        (
            "set",
            "globalThis.held='x'.repeat(300000);var items=new Set();",
            "for(var i=0;i<6;i++)items.add(String(i)+'y'.repeat(100000));",
        ),
        (
            "array",
            "globalThis.held='x'.repeat(300000);var items=[];",
            "for(var i=0;i<6;i++)items.push('y'.repeat(100000));",
        ),
    ] {
        let mut runtime = page(2 * 1024 * 1024);
        let source = format!(
            "globalThis.seed='s'.repeat(100000);var r=new ShadowRealm();r.evaluate({:?});",
            format!("{setup}0;")
        );
        execute(&mut runtime, 1, &source).unwrap();
        let source = format!("r.evaluate({:?});", format!("{growth}0;"));
        let result = execute(&mut runtime, 1, &source);
        assert!(
            result.is_err(),
            "{name} bypassed aggregate admission: {result:?}"
        );
        assert!(
            execute(&mut runtime, 1, "42;").is_err(),
            "{name} was not a terminal document failure"
        );
        assert!(
            runtime.realm_stats(1).unwrap().document_heap_bytes <= 2 * 1024 * 1024,
            "{name} committed excess managed storage"
        );
    }
}

#[test]
fn shrinking_a_live_child_buffer_releases_its_document_charge() {
    let mut runtime = page(2 * 1024 * 1024);
    execute(&mut runtime, 1, "var r=new ShadowRealm();r.evaluate('var b=new ArrayBuffer(600000,{maxByteLength:1000000});0;');").unwrap();
    let before = runtime.realm_stats(1).unwrap().document_heap_bytes;
    execute(&mut runtime, 1, "r.evaluate('b.resize(100000);0;');").unwrap();
    assert!(runtime.realm_stats(1).unwrap().document_heap_bytes <= before - 500000);
    execute(&mut runtime, 1, "r=null;0;").unwrap();
    let stats = runtime.realm_stats(1).unwrap();
    assert_eq!(stats.document_heap_bytes, stats.heap.managed_bytes);
}

#[test]
fn unchanged_live_managed_storage_does_not_collect_at_every_small_call() {
    let mut counts = Vec::new();
    for calls in [1, 1000] {
        let mut runtime = page(2 * 1024 * 1024);
        execute(
            &mut runtime,
            1,
            "globalThis.held='x'.repeat(600000);function noop(){}",
        )
        .unwrap();
        let before = runtime.realm_stats(1).unwrap().heap.major_collections;
        execute(
            &mut runtime,
            1,
            &format!("for(var i=0;i<{calls};i++)noop();0;"),
        )
        .unwrap();
        counts.push(runtime.realm_stats(1).unwrap().heap.major_collections - before);
    }
    assert_eq!(counts[0], counts[1]);
}

#[test]
fn nested_shadow_realms_share_the_same_terminal_document_budget() {
    let mut runtime = page(2 * 1024 * 1024);
    let inner="var realms=[];for(var i=0;i<16;i++){var child=new ShadowRealm();child.evaluate(\"globalThis.held='x'.repeat(300000);0;\");realms.push(child);}0;";
    let result = execute(
        &mut runtime,
        1,
        &format!("new ShadowRealm().evaluate({inner:?});"),
    );
    assert!(result.is_err());
    assert!(runtime.run_microtask_checkpoint(1).is_err());
    assert!(execute(&mut runtime, 1, "42;").is_err());
    assert!(runtime.realm_stats(1).unwrap().document_heap_bytes <= 2 * 1024 * 1024);
}

#[test]
fn queued_cleanup_keeps_its_managed_heap_until_delivery() {
    let mut runtime = page(2 * 1024 * 1024);
    execute(&mut runtime,1,"new ShadowRealm().evaluate(\"globalThis.held='x'.repeat(300000);var registry=new FinalizationRegistry(()=>{globalThis.held=null;});registry.register({},0);0;\");0;").unwrap();
    let before = runtime.realm_stats(1).unwrap();
    assert_eq!(before.scheduling.pending_browser_tasks, 1);
    assert!(before.document_heap_bytes >= before.heap.managed_bytes + 600000);
    assert_eq!(runtime.run_task_turn(1, 10, 1).unwrap().tasks_run, 1);
    execute(&mut runtime, 1, "0;").unwrap();
    let after = runtime.realm_stats(1).unwrap();
    assert_eq!(after.document_heap_bytes, after.heap.managed_bytes);
}

#[test]
fn realm_entry_reclaims_dead_caller_storage_before_large_child_growth() {
    for wrapped in [false, true] {
        let mut runtime = page(4 * 1024 * 1024);
        let growth = "globalThis.extra='y'.repeat(1200000);0;";
        let setup = format!("globalThis.held='x'.repeat(400000);()=>{{{growth}}};");
        let invoke = if wrapped {
            "grow();".to_string()
        } else {
            format!("r.evaluate({growth:?});")
        };
        let source=format!("var r=new ShadowRealm();var grow=r.evaluate({setup:?});function poll(){{var junk={{value:'x'.repeat(300000)}};junk=null;{invoke}return 42;}}poll();");
        assert_eq!(
            execute(&mut runtime, 1, &source).unwrap(),
            Value::Number(42.0)
        );
        assert!(runtime.realm_stats(1).unwrap().document_heap_bytes <= 4 * 1024 * 1024);
    }
}
