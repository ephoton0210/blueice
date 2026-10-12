// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn vm() -> Vm {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    config.heap.major_threshold_bytes = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_web_file_api().unwrap();
    vm
}
fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}
fn jobs(vm: &mut Vm) {
    vm.run_browser_tasks_bounded(256).unwrap();
}

#[test]
fn for_await_consumes_actual_chunks_and_releases_the_lock_at_eof() {
    let mut vm = vm();
    run(&mut vm,"let values=[],canceled=0;const stream=new ReadableStream({start(c){c.enqueue(1);c.enqueue(2);c.close();},cancel(){canceled++;}});(async()=>{for await(const value of stream)values.push(value);})();");
    jobs(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "values.join(',')==='1,2' && !stream.locked && canceled===0"
        ),
        Value::Bool(true)
    );
}

#[test]
fn for_await_break_cancels_and_waits_for_the_source_before_leaving() {
    let mut vm = vm();
    run(&mut vm,"let called=0,reason='unset',finish,left=false;const stream=new ReadableStream({start(c){c.enqueue(7);},cancel(value){called++;reason=value;return new Promise(resolve=>finish=resolve);}});(async()=>{for await(const value of stream){break;}left=true;})();");
    jobs(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "called===1 && reason===undefined && !stream.locked && !left"
        ),
        Value::Bool(true)
    );
    run(&mut vm, "finish();true");
    jobs(&mut vm);
    assert_eq!(run(&mut vm, "left"), Value::Bool(true));
}

#[test]
fn prevent_cancel_releases_the_lock_and_preserves_unread_data() {
    let mut vm = vm();
    run(&mut vm,"let called=0,first,second;const stream=new ReadableStream({start(c){c.enqueue(1);c.enqueue(2);},cancel(){called++;}});(async()=>{for await(const value of stream.values({preventCancel:true})){first=value;break;}second=(await stream.getReader().read()).value;})();");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "first===1 && second===2 && called===0"),
        Value::Bool(true)
    );
}

#[test]
fn promise_chunks_and_return_arguments_keep_their_original_identity() {
    let mut vm = vm();
    run(&mut vm,"const payload=Promise.resolve(7),reason=Promise.resolve(9);let chunk,returned,canceled;const stream=new ReadableStream({start(c){c.enqueue(payload);},cancel(value){canceled=value;}}),iterator=stream.values();iterator.next().then(r=>chunk=r.value);iterator.return(reason).then(r=>returned=r); ");
    jobs(&mut vm);
    assert_eq!(run(&mut vm,"chunk===payload && returned.value===reason && returned.done && canceled===reason && !stream.locked"),Value::Bool(true));
}

#[test]
fn queued_next_and_return_wait_for_the_previous_read_in_order() {
    let mut vm = vm();
    run(&mut vm,"let controller,canceled=0,reason,events=[];const stream=new ReadableStream({start(c){controller=c;},cancel(value){canceled++;reason=value;}}),iterator=stream.values();iterator.next().then(r=>events.push(r.value));iterator.next().then(r=>events.push(r.value));iterator.return('stop').then(r=>events.push(r.value));iterator.next().then(r=>events.push(r.done));");
    jobs(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "events.length===0 && canceled===0 && stream.locked"
        ),
        Value::Bool(true)
    );
    run(&mut vm, "controller.enqueue(1);true");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "events.join(',')==='1' && canceled===0"),
        Value::Bool(true)
    );
    run(&mut vm, "controller.enqueue(2);true");
    jobs(&mut vm);
    assert_eq!(run(&mut vm,"events.join(',')==='1,2,stop,true' && canceled===1 && reason==='stop' && !stream.locked"),Value::Bool(true));
}

#[test]
fn source_errors_reject_once_release_the_lock_and_finish_queued_calls() {
    let mut vm = vm();
    run(&mut vm,"const error={message:'source'};let controller,seen,done=0,canceled=0;const stream=new ReadableStream({start(c){controller=c;},cancel(){canceled++;}}),iterator=stream.values();iterator.next().catch(e=>seen=e);iterator.next().then(r=>{if(r.done)done++;});controller.error(error);");
    jobs(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "seen===error && done===1 && !stream.locked && canceled===0"
        ),
        Value::Bool(true)
    );
}

#[test]
fn cancellation_rejection_keeps_identity_and_the_iterator_finishes_unlocked() {
    let mut vm = vm();
    run(&mut vm,"const error={message:'cancel'};let seen,done=false;const stream=new ReadableStream({cancel(){return Promise.reject(error);}}),iterator=stream.values();iterator.return(7).catch(e=>seen=e);iterator.next().then(r=>done=r.done);");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "seen===error && done && !stream.locked"),
        Value::Bool(true)
    );
}

#[test]
fn invalid_iterator_receivers_reject_promises_and_values_rejects_locked_streams() {
    let mut vm = vm();
    run(&mut vm,"let invalid=0,locked=false,conversion=false;const stream=new ReadableStream(),iterator=stream.values();const prototype=Object.getPrototypeOf(iterator);for(const value of [null,{},new EventTarget()]){prototype.next.call(value).catch(e=>{if(e instanceof TypeError)invalid++;});prototype.return.call(value).catch(e=>{if(e instanceof TypeError)invalid++;});}try{stream.values();}catch(e){locked=e instanceof TypeError;}try{new ReadableStream().values(7);}catch(e){conversion=e instanceof TypeError;}");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "invalid===6 && locked && conversion"),
        Value::Bool(true)
    );
}

#[test]
fn iterator_prototype_aliases_and_internal_reads_ignore_public_method_overrides() {
    let mut vm = vm();
    run(&mut vm,"let result;const stream=new ReadableStream({start(c){c.enqueue('internal');c.close();}});stream.getReader=()=>{throw 'public';};const iterator=stream.values(),prototype=Object.getPrototypeOf(iterator);iterator.next().then(r=>result=r.value);let shape=ReadableStream.prototype[Symbol.asyncIterator]===ReadableStream.prototype.values && iterator[Symbol.asyncIterator]()===iterator && Object.prototype.toString.call(iterator)==='[object ReadableStream AsyncIterator]' && prototype.next.length===0 && prototype.return.length===1 && iterator.throw===undefined;");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "shape && result==='internal'"),
        Value::Bool(true)
    );
}

#[test]
fn a_pending_iteration_keeps_its_callbacks_and_bytes_alive_through_collection() {
    let mut vm = vm();
    run(&mut vm,"let controller,pending,result;(()=>{const stream=new ReadableStream({start(c){controller=c;}});pending=stream.values().next();})();for(let i=0;i<100;i++){new EventTarget();}pending.then(r=>result=r.value);controller.enqueue({text:'retained 中文'});controller.close();");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "result.text==='retained 中文'"),
        Value::Bool(true)
    );
}

#[test]
fn for_await_return_waits_and_preserves_its_value_after_cancellation() {
    let mut vm = vm();
    run(&mut vm,"let finish,answer,called=0;const stream=new ReadableStream({start(c){c.enqueue(1);},cancel(){called++;return new Promise(resolve=>finish=resolve);}});(async()=>{for await(const value of stream){return 9;}})().then(value=>answer=value);");
    jobs(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "called===1 && answer===undefined && !stream.locked"
        ),
        Value::Bool(true)
    );
    run(&mut vm, "finish();true");
    jobs(&mut vm);
    assert_eq!(run(&mut vm, "answer===9"), Value::Bool(true));
}

#[test]
fn for_await_throw_waits_for_cancel_and_keeps_the_original_throw() {
    let mut vm = vm();
    run(&mut vm,"const original={message:'body'},cancelError={message:'cancel'};let finish,seen;const stream=new ReadableStream({start(c){c.enqueue(1);},cancel(){return new Promise((resolve,reject)=>finish=()=>reject(cancelError));}});(async()=>{for await(const value of stream){throw original;}})().catch(e=>seen=e);");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "seen===undefined && !stream.locked"),
        Value::Bool(true)
    );
    run(&mut vm, "finish();true");
    jobs(&mut vm);
    assert_eq!(run(&mut vm, "seen===original"), Value::Bool(true));
}

#[test]
fn for_await_break_reports_cancellation_failure_to_its_enclosing_catch() {
    let mut vm = vm();
    run(&mut vm,"const error={message:'cancel'};let seen;const stream=new ReadableStream({start(c){c.enqueue(1);},cancel(){return Promise.reject(error);}});(async()=>{try{for await(const value of stream){break;}}catch(e){seen=e;}})();");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "seen===error && !stream.locked"),
        Value::Bool(true)
    );
}

#[test]
fn failed_async_next_does_not_call_return_and_primitive_return_is_rejected() {
    let mut vm = vm();
    run(&mut vm,"const error={message:'next'};let called=0,seen,primitive;const failing={[Symbol.asyncIterator](){return this;},next(){return Promise.reject(error);},return(){called++;return Promise.resolve({done:true});}};(async()=>{try{for await(const value of failing){}}catch(e){seen=e;}})();const invalid={[Symbol.asyncIterator](){return this;},next(){return Promise.resolve({value:1,done:false});},return(){return Promise.resolve(7);}};(async()=>{try{for await(const value of invalid){break;}}catch(e){primitive=e instanceof TypeError;}})();");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "seen===error && called===0 && primitive"),
        Value::Bool(true)
    );
}

#[test]
fn labeled_continue_awaits_inner_return_before_the_next_outer_iteration() {
    let mut vm = vm();
    run(&mut vm,"let finish,events=[];const inner={[Symbol.asyncIterator](){return this;},next(){return Promise.resolve({value:1,done:false});},return(){events.push('close');return new Promise(resolve=>finish=()=>resolve({done:true}));}};(async()=>{outer:for(const n of [1,2]){events.push(n);for await(const value of inner){continue outer;}}events.push('done');})();");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "events.join(',')==='1,close'"),
        Value::Bool(true)
    );
    run(&mut vm, "finish();true");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "events.join(',')==='1,close,2,close'"),
        Value::Bool(true)
    );
    run(&mut vm, "finish();true");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "events.join(',')==='1,close,2,close,done'"),
        Value::Bool(true)
    );
}

#[test]
fn finished_iterators_do_not_retain_unobserved_return_values_after_collection() {
    let mut vm = vm();
    run(
        &mut vm,
        "const iterator=new ReadableStream({start(c){c.close();}}).values();iterator.next();true",
    );
    jobs(&mut vm);
    run(&mut vm,"var weak;(()=>{const value={message:'temporary'};weak=new WeakRef(value);iterator.return(value);})();true");
    jobs(&mut vm);
    run(&mut vm, "for(let i=0;i<100;i++){new EventTarget();}true");
    assert_eq!(run(&mut vm, "weak.deref()===undefined"), Value::Bool(true));
}

#[test]
fn next_ignores_its_argument_without_retaining_it_during_a_pending_read() {
    let mut vm = vm();
    run(&mut vm,"let controller;const iterator=new ReadableStream({start(c){controller=c;}}).values();var weak;(()=>{const ignored={message:'unused'};weak=new WeakRef(ignored);iterator.next(ignored);})();true");
    run(&mut vm, "for(let i=0;i<100;i++){new EventTarget();}true");
    assert_eq!(run(&mut vm, "weak.deref()===undefined"), Value::Bool(true));
}
