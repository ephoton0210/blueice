// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Behavioral reference: https://streams.spec.whatwg.org/#rs-tee
use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};
fn vm() -> Vm {
    vm_with_budget(VmConfig::default().instruction_budget)
}
fn vm_with_budget(budget: u64) -> Vm {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    config.instruction_budget = budget;
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
fn tee_locks_the_source_preserves_generic_chunk_identity_and_uses_native_branch_prototypes() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        class Source extends ReadableStream {}
        const payload={value:7},source=new Source({start(c){c.enqueue(payload);c.close();}});
        const branches=source.tee(),left=branches[0].getReader(),right=branches[1].getReader();let a,b,done=0,rejected=0;
        left.read().then(r=>a=r.value);right.read().then(r=>b=r.value);
        left.read().then(r=>{if(r.done)done++;});right.read().then(r=>{if(r.done)done++;});
        for(const action of [()=>source.tee(),()=>ReadableStream.prototype.tee.call({}),()=>new source.tee()]){
            try{action();}catch(e){if(e instanceof TypeError)rejected++;}
        }
    "#,
    );
    jobs(&mut vm);
    assert_eq!(run(&mut vm,"source instanceof Source && source.locked && branches.length===2 && branches[0] instanceof ReadableStream && !(branches[0] instanceof Source) && a===payload && b===payload && done===2 && rejected===3"),Value::Bool(true));
}

#[test]
fn fast_branch_can_read_ahead_and_the_slow_branch_keeps_every_chunk() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        let pulls=0,fast=[],slow=[];
        const source=new ReadableStream({pull(c){c.enqueue(++pulls);if(pulls===4)c.close();}});
        const branches=source.tee();
        function drain(reader,values){return reader.read().then(r=>{if(r.done)return;values.push(r.value);return drain(reader,values);});}
        drain(branches[0].getReader(),fast);
    "#,
    );
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "fast.join(',')==='1,2,3,4' && pulls===4"),
        Value::Bool(true)
    );
    run(&mut vm, "drain(branches[1].getReader(),slow)");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "slow.join(',')==='1,2,3,4'"),
        Value::Bool(true)
    );
}

#[test]
fn cancelling_both_branches_waits_for_one_source_cancellation_with_ordered_reasons() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        let called=0,reasons,finish,settled=0;const one={id:1},two={id:2};
        const source=new ReadableStream({cancel(value){called++;reasons=value;return new Promise(resolve=>finish=resolve);}});
        const branches=source.tee();branches[1].cancel(two).then(()=>settled++);
    "#,
    );
    jobs(&mut vm);
    assert_eq!(run(&mut vm, "called===0 && settled===0"), Value::Bool(true));
    run(&mut vm, "branches[0].cancel(one).then(()=>settled++)");
    jobs(&mut vm);
    assert_eq!(run(&mut vm,"called===1 && reasons.length===2 && reasons[0]===one && reasons[1]===two && settled===0"),Value::Bool(true));
    run(&mut vm, "finish()");
    jobs(&mut vm);
    assert_eq!(run(&mut vm, "settled===2"), Value::Bool(true));
}

#[test]
fn cancelling_one_branch_does_not_cancel_the_other_and_resolves_at_source_eof() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        let cancelCalls=0,cancelDone=false,values=[];
        const source=new ReadableStream({start(c){c.enqueue('kept');c.close();},cancel(){cancelCalls++;}});
        const branches=source.tee();branches[0].cancel('left').then(()=>cancelDone=true);
        const reader=branches[1].getReader();reader.read().then(r=>values.push(r.value));reader.read().then(r=>values.push(r.done));
    "#,
    );
    jobs(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "cancelDone && cancelCalls===0 && values.join(',')==='kept,true'"
        ),
        Value::Bool(true)
    );
}

#[test]
fn source_error_precedes_queued_chunk_delivery_and_keeps_exact_reason_identity() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        const reason={message:'source error'};let first,second,seen=0;
        const source=new ReadableStream({pull(c){c.enqueue('discarded');Promise.resolve().then(()=>c.error(reason));}},{highWaterMark:0});
        const branches=source.tee();branches[0].getReader().read().then(()=>seen++,e=>first=e);
        branches[1].getReader().read().then(()=>seen++,e=>second=e);
    "#,
    );
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "seen===0 && first===reason && second===reason"),
        Value::Bool(true)
    );
}

#[test]
fn byte_tee_copies_buffers_between_default_readers() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        const source=new Blob([new Uint8Array([1,0,255,3])]).stream();const branches=source.tee();let a,b;
        branches[0].getReader().read().then(r=>a=r.value);branches[1].getReader().read().then(r=>b=r.value);
    "#,
    );
    jobs(&mut vm);
    assert_eq!(run(&mut vm,"a instanceof Uint8Array && b instanceof Uint8Array && a.buffer!==b.buffer && Array.from(a).join(',')==='1,0,255,3' && Array.from(b).join(',')==='1,0,255,3';a[0]=9;b[0]===1"),Value::Bool(true));
}

#[test]
fn byte_tee_forwards_real_byob_demand_and_closes_both_reader_kinds() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        let missing=false,a,b,done=0;
        const source=new ReadableStream({type:'bytes',pull(c){const request=c.byobRequest;
            if(!request){missing=true;c.error('BYOB required');return;}request.view.set(new Uint8Array([7,0,255]));request.respond(3);c.close();}});
        const branches=source.tee(),left=branches[0].getReader({mode:'byob'}),right=branches[1].getReader();
        const original=new Uint8Array(8);left.read(original).then(r=>a=r.value);right.read().then(r=>b=r.value);
    "#,
    );
    jobs(&mut vm);
    assert_eq!(run(&mut vm,"!missing && original.buffer.byteLength===0 && Array.from(a).join(',')==='7,0,255' && Array.from(b).join(',')==='7,0,255' && a.buffer!==b.buffer"),Value::Bool(true));
    run(&mut vm,"left.read(new Uint8Array(8)).then(r=>{if(r.done && r.value.byteLength===0)done++;});right.read().then(r=>{if(r.done)done++;})");
    jobs(&mut vm);
    assert_eq!(run(&mut vm, "done===2"), Value::Bool(true));
}

#[test]
fn byte_tee_preserves_byob_element_alignment_across_partial_source_responses() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        let index=0,result;const source=new ReadableStream({type:'bytes',pull(c){const request=c.byobRequest;
            request.view[0]=++index;request.respond(1);if(index===2)c.close();}});
        const branches=source.tee();branches[0].getReader({mode:'byob'}).read(new Uint16Array(2)).then(r=>result=r.value);
    "#,
    );
    jobs(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "index===2 && result instanceof Uint16Array && result.length===1 && result[0]===513"
        ),
        Value::Bool(true)
    );
}

#[test]
fn source_cancellation_rejection_reaches_both_branch_cancel_promises() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        const reason={failure:'cancel'};let a,b;
        const branches=new ReadableStream({cancel(){return Promise.reject(reason);}}).tee();
        branches[0].cancel('left').catch(e=>a=e);branches[1].cancel('right').catch(e=>b=e);
    "#,
    );
    jobs(&mut vm);
    assert_eq!(run(&mut vm, "a===reason && b===reason"), Value::Bool(true));
}

#[test]
fn source_error_resolves_one_canceled_branch_while_rejecting_the_other() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        let controller,cancelDone=false,error,closed;const reason={failure:'read'};
        const branches=new ReadableStream({start(c){controller=c;}}).tee();
        branches[0].cancel('left').then(()=>cancelDone=true);const reader=branches[1].getReader();
        reader.read().catch(e=>error=e);reader.closed.catch(e=>closed=e);
    "#,
    );
    jobs(&mut vm);
    run(&mut vm, "controller.error(reason)");
    jobs(&mut vm);
    assert_eq!(
        run(&mut vm, "cancelDone && error===reason && closed===reason"),
        Value::Bool(true)
    );
}

#[test]
fn byte_tee_can_cancel_the_requesting_branch_during_a_live_source_read() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        let request,controller,leftDone=false,right,canceled=false;
        const source=new ReadableStream({type:'bytes',pull(c){controller=c;request=c.byobRequest;}});
        const branches=source.tee(),left=branches[0].getReader({mode:'byob'}),other=branches[1].getReader();
        left.read(new Uint8Array(8)).then(r=>leftDone=r.done && r.value===undefined);
        other.read().then(r=>right=r.value);
    "#,
    );
    jobs(&mut vm);
    run(&mut vm,"left.cancel('gone').then(()=>canceled=true);request.view.set(new Uint8Array([4,5]));request.respond(2);controller.close();");
    jobs(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "leftDone && Array.from(right).join(',')==='4,5' && !canceled"
        ),
        Value::Bool(true)
    );
    run(&mut vm, "other.read()");
    jobs(&mut vm);
    assert_eq!(run(&mut vm, "canceled"), Value::Bool(true));
}

#[test]
fn byte_tee_switches_between_byob_and_default_source_readers_without_stale_errors() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        let index=0,left=[],right=[];
        const branches=new ReadableStream({type:'bytes',pull(c){const value=++index;if(c.byobRequest){const r=c.byobRequest;r.view[0]=value;r.respond(1);}else{c.enqueue(new Uint8Array([value]));}if(value===3)c.close();}}).tee();
        const a=branches[0].getReader({mode:'byob'}),b=branches[1].getReader();
        a.read(new Uint8Array(8)).then(r=>left.push(r.value[0]));
    "#,
    );
    jobs(&mut vm);
    run(
        &mut vm,
        "b.read().then(r=>right.push(r.value[0]));b.read().then(r=>right.push(r.value[0]));",
    );
    jobs(&mut vm);
    run(&mut vm,"a.read(new Uint8Array(8)).then(r=>left.push(r.value[0]));a.read(new Uint8Array(8)).then(r=>left.push(r.value[0]));b.read().then(r=>right.push(r.value[0]));");
    jobs(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "left.join(',')==='1,2,3' && right.join(',')==='1,2,3'"
        ),
        Value::Bool(true)
    );
}

#[test]
fn queued_tee_chunks_and_source_callbacks_survive_collection_without_source_references() {
    let mut vm = vm();
    run(
        &mut vm,
        r#"
        let reader,seen;(()=>{const source=new ReadableStream({start(c){c.enqueue({message:'retained'});c.close();}});reader=source.tee()[1].getReader();})();
        for(let i=0;i<100;i++){new EventTarget();}
        reader.read().then(r=>seen=r.value.message);
    "#,
    );
    jobs(&mut vm);
    assert_eq!(run(&mut vm, "seen==='retained'"), Value::Bool(true));
}

#[test]
fn a_slow_branch_queue_limit_errors_the_tee_and_cancels_the_source() {
    let mut vm = vm_with_budget(10_000_000);
    run(
        &mut vm,
        r#"
        let pulls=0,canceled=0,error,cancelReason;
        const source=new ReadableStream({pull(c){c.enqueue(++pulls);},cancel(reason){canceled++;cancelReason=reason;}});
        const branches=source.tee(),reader=branches[0].getReader();
        function drain(){reader.read().then(r=>{if(!r.done)drain();},e=>error=e);}
        drain();
    "#,
    );
    vm.run_promise_jobs_bounded(200_000).unwrap();
    assert_eq!(
        run(
            &mut vm,
            "error instanceof RangeError && canceled===1 && cancelReason===error"
        ),
        Value::Bool(true)
    );
}
