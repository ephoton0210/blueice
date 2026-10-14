// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn vm() -> Vm {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    config.heap.major_threshold_bytes = 1;
    Vm::new(config).unwrap()
}

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

fn collect(vm: &mut Vm) {
    run(vm, "for(let i=0;i<100;i++){({value:i});} true");
}

#[test]
fn unreachable_settled_promises_and_their_results_can_be_collected() {
    let mut vm = vm();
    run(&mut vm, "var weakPromise,weakResult;(()=>{const result={value:7},promise=Promise.resolve(result);weakPromise=new WeakRef(promise);weakResult=new WeakRef(result);})();true");
    collect(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "weakPromise.deref()===undefined && weakResult.deref()===undefined"
        ),
        Value::Bool(true)
    );
}

#[test]
fn unreachable_promise_result_cycles_can_be_collected() {
    let mut vm = vm();
    run(&mut vm, "var weakPromise,weakResult;(()=>{const result={},promise=Promise.resolve(result);result.promise=promise;weakPromise=new WeakRef(promise);weakResult=new WeakRef(result);})();true");
    collect(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "weakPromise.deref()===undefined && weakResult.deref()===undefined"
        ),
        Value::Bool(true)
    );
}

#[test]
fn unreachable_pending_promise_reaction_cycles_can_be_collected() {
    let mut vm = vm();
    run(&mut vm, "var weakPromise,weakTarget;(()=>{const promise=new Promise(()=>{}),target=promise.then(()=>promise);weakPromise=new WeakRef(promise);weakTarget=new WeakRef(target);})();true");
    collect(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "weakPromise.deref()===undefined && weakTarget.deref()===undefined"
        ),
        Value::Bool(true)
    );
}

#[test]
fn an_old_promise_traces_new_reaction_targets_during_minor_collection() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    run(
        &mut vm,
        "var finish,observed;const promise=new Promise(resolve=>finish=resolve);true",
    );
    collect(&mut vm);
    run(
        &mut vm,
        "promise.then(value=>({value})).then(value=>observed=value.value);true",
    );
    collect(&mut vm);
    run(&mut vm, "finish(7);true");
    vm.run_promise_jobs_bounded(64).unwrap();
    assert_eq!(run(&mut vm, "observed===7"), Value::Bool(true));
}

#[test]
fn a_live_promise_preserves_its_result_and_later_reactions() {
    let mut vm = vm();
    run(&mut vm, "var livePromise,weakResult,observed;(()=>{const result={value:7};livePromise=Promise.resolve(result);weakResult=new WeakRef(result);})();true");
    collect(&mut vm);
    run(&mut vm, "livePromise.then(value=>observed=value);true");
    vm.run_promise_jobs_bounded(64).unwrap();
    assert_eq!(
        run(
            &mut vm,
            "observed===weakResult.deref() && observed.value===7"
        ),
        Value::Bool(true)
    );
}

#[test]
fn async_host_wait_keeps_its_promise_alive_until_the_vm_settles_it() {
    let mut vm = vm();
    vm.install_test262_done().unwrap();
    run(&mut vm, "var view=new Int32Array(new SharedArrayBuffer(4)),weakWait,observed;(()=>{const promise=Atomics.waitAsync(view,0,0,10000).value;weakWait=new WeakRef(promise);promise.then(status=>{observed=status;$DONE();});})();true");
    collect(&mut vm);
    assert_eq!(
        run(&mut vm, "weakWait.deref() instanceof Promise"),
        Value::Bool(true)
    );
    assert_eq!(run(&mut vm, "Atomics.notify(view,0)"), Value::Number(1.0));
    assert_eq!(vm.run_test262_async_until_done(), Ok(Some(Ok(()))));
    assert_eq!(run(&mut vm, "observed==='ok'"), Value::Bool(true));
    collect(&mut vm);
    assert_eq!(
        run(&mut vm, "weakWait.deref()===undefined"),
        Value::Bool(true)
    );
}

#[test]
fn async_host_wait_settles_inline_int32_and_bigint_promise_batches_after_collection() {
    for (array, expected) in [("Int32Array", "0"), ("BigInt64Array", "0n")] {
        let mut vm = vm();
        vm.install_test262_done().unwrap();
        run(&mut vm, &format!("var view=new {array}(new SharedArrayBuffer(8)),observed;var valueHook={{valueOf(){{return undefined;}}}},primitiveHook={{[Symbol.toPrimitive](){{return undefined;}}}};Promise.all([Atomics.waitAsync(view,0,{expected}).value,Atomics.waitAsync(view,0,{expected},undefined).value,Atomics.waitAsync(view,0,{expected},valueHook).value,Atomics.waitAsync(view,0,{expected},primitiveHook).value]).then(statuses=>{{observed=statuses.join(',');$DONE();}},$DONE);true"));
        collect(&mut vm);
        assert_eq!(run(&mut vm, "Atomics.notify(view,0)"), Value::Number(4.0));
        assert_eq!(vm.run_test262_async_until_done(), Ok(Some(Ok(()))));
        assert_eq!(run(&mut vm, "observed==='ok,ok,ok,ok'"), Value::Bool(true));
    }
}

#[test]
fn async_host_wait_original_test262_timeout_fixtures_settle_in_both_script_modes() {
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/reference/test262/test/built-ins/Atomics/waitAsync");
    for file in [
        "undefined-for-timeout.js",
        "bigint/undefined-for-timeout.js",
    ] {
        let source = std::fs::read_to_string(corpus.join(file)).unwrap();
        for strict in [false, true] {
            let mut vm = Vm::new(VmConfig {
                instruction_budget: 100_000,
                ..VmConfig::default()
            })
            .unwrap();
            vm.install_test262_harness().unwrap();
            vm.install_test262_done().unwrap();
            let text = if strict {
                format!("\"use strict\";\n{source}")
            } else {
                source.clone()
            };
            run(&mut vm, &text);
            assert_eq!(
                vm.run_test262_async_until_done(),
                Ok(Some(Ok(()))),
                "{file}, strict={strict}"
            );
        }
    }
}

#[test]
fn a_retained_resolver_preserves_pending_reactions_and_their_target() {
    let mut vm = vm();
    run(&mut vm, "var finish,weakPromise,weakTarget,observed;(()=>{const promise=new Promise(resolve=>finish=resolve),target=promise.then(value=>({value}));target.then(value=>observed=value.value);weakPromise=new WeakRef(promise);weakTarget=new WeakRef(target);})();true");
    collect(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "weakPromise.deref()!==undefined && weakTarget.deref()!==undefined"
        ),
        Value::Bool(true)
    );
    run(&mut vm, "finish(7);true");
    vm.run_promise_jobs_bounded(64).unwrap();
    assert_eq!(run(&mut vm, "observed===7"), Value::Bool(true));
}
