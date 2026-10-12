// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, HeapConfig, Value, Vm, VmConfig};
fn web_vm() -> Vm {
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    vm
}
fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn abort_controller_owns_one_readonly_signal_and_preserves_exact_reason() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        const controller=new AbortController(), signal=controller.signal, reason={message:'stop'};
        const initial=signal instanceof AbortSignal && signal instanceof EventTarget && !signal.aborted
            && signal.reason === undefined && controller.signal === signal && signal.onabort === null;
        signal.aborted=true; signal.reason='forged'; controller.signal={};
        let count=0, trust=false;
        signal.addEventListener('abort',event=>{count++;trust=event instanceof Event && event.isTrusted
            && event.target === signal && event.currentTarget === signal && !event.bubbles && !event.cancelable;});
        controller.abort(reason); controller.abort('replacement');
        let thrown=false;try{signal.throwIfAborted();}catch(error){thrown=error===reason;}
        initial && controller.signal === signal && signal.aborted && signal.reason===reason && count===1 && trust && thrown
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn abort_static_methods_and_illegal_receivers_keep_native_brands() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        const one=AbortSignal.abort(), two=AbortSignal.abort(undefined), primitive=AbortSignal.abort(null);
        let rejected=0;
        for(const action of [()=>AbortController(),()=>new AbortSignal(),()=>AbortSignal(),
            ()=>AbortController.prototype.abort.call({}),()=>AbortSignal.prototype.throwIfAborted.call({}),
            ()=>AbortSignal.any([{}])]) {
            try{action();}catch(error){if(error instanceof TypeError) rejected++;}
        }
        one!==two && one.aborted && one.reason instanceof DOMException && one.reason.name==='AbortError'
            && one.reason.code===20 && primitive.reason===null && rejected===6
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn listener_signals_remove_callbacks_before_abort_event_dispatch() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        const target=new EventTarget(), controller=new AbortController(), events=[];
        target.addEventListener('data',()=>events.push('data'),{signal:controller.signal});
        controller.signal.onabort=()=>{target.dispatchEvent(new Event('data'));events.push('abort');};
        target.dispatchEvent(new Event('data'));controller.abort();target.dispatchEvent(new Event('data'));
        target.addEventListener('data',()=>events.push('late'),{signal:controller.signal});
        target.dispatchEvent(new Event('data'));
        events.join(',')==='data,abort'
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn abort_any_sets_all_dependent_reasons_before_firing_source_handlers() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        const a=new AbortController(), b=new AbortController(), reason={code:7}, events=[];
        const first=AbortSignal.any([a.signal,b.signal,a.signal]);
        const second=AbortSignal.any([first,b.signal]);
        a.signal.addEventListener('abort',()=>events.push('source:'+first.aborted+':'+second.aborted));
        first.addEventListener('abort',()=>events.push('first'));
        second.addEventListener('abort',()=>events.push('second'));
        a.abort(reason);b.abort('ignored');
        const already=AbortSignal.any([AbortSignal.abort('first'),AbortSignal.abort('second')]);
        first.reason===reason && second.reason===reason && already.reason==='first'
            && !AbortSignal.any([]).aborted && events.join(',')==='source:true:true,first,second'
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn dependent_signals_and_listener_reasons_survive_small_nursery_collection() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 1,
            max_heap_bytes: 1_000_000,
        },
        ..VmConfig::default()
    })
    .unwrap();
    vm.install_web_file_api().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
        const controller=new AbortController(), reason={retained:'reason'};let observed='';
        (()=>{const combined=AbortSignal.any([controller.signal]);combined.addEventListener('abort',event=>{observed=event.target.reason.retained;});})();
        for(let index=0;index<100;index++){new EventTarget();}
        controller.abort(reason);observed==='reason'
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn timeout_queues_a_browser_task_and_uses_timeout_error() {
    let mut vm = web_vm();
    assert_eq!(
        run(
            &mut vm,
            r#"
        const signal=AbortSignal.timeout(0);let fired=0;signal.onabort=()=>fired++;
        let rejected=0;for(const value of [-1,Infinity,NaN,18446744073709551616]){
            try{AbortSignal.timeout(value);}catch(error){if(error instanceof TypeError) rejected++;}
        }
        !signal.aborted && fired===0 && rejected===4
    "#
        ),
        Value::Bool(true)
    );
    vm.run_browser_tasks_bounded(64).unwrap();
    assert_eq!(run(&mut vm,"signal.aborted && signal.reason instanceof DOMException && signal.reason.name === 'TimeoutError' && fired === 1"),Value::Bool(true));
}
