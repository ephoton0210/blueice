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
