// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Behavioral references: https://streams.spec.whatwg.org/
// and https://w3c.github.io/FileAPI/#stream-method
use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}
fn web_vm() -> Vm {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_web_file_api().unwrap();
    vm
}
fn checkpoint(vm: &mut Vm) {
    vm.run_browser_tasks_bounded(256).unwrap();
}

#[test]
fn stream_readers_lock_and_release_and_preserve_real_queued_values() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller, secondRejected=false, results=[];
        const stream=new ReadableStream({start(c){controller=c;c.enqueue({value:'owned'});}});
        const reader=stream.getReader();
        try{stream.getReader();}catch(e){secondRejected=e instanceof TypeError;}
        reader.read().then(r=>results.push(r.value.value+':'+r.done));
        controller.close();
        reader.read().then(r=>results.push(String(r.value)+':'+r.done));
        reader.closed.then(()=>results.push('closed'));
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(run(&mut vm, "stream.locked && secondRejected && results.includes('owned:false') && results.includes('undefined:true') && results.includes('closed')"), Value::Bool(true));
    assert_eq!(
        run(&mut vm, "reader.releaseLock(); !stream.locked"),
        Value::Bool(true)
    );
}

#[test]
fn stream_default_controller_backpressure_calls_pull_only_when_needed() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller,pulls=0,values=[];
        const source={start(c){controller=c;c.enqueue('initial');},pull(c){pulls++;c.enqueue('next');}};
        const stream=new ReadableStream(source,{highWaterMark:1});
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "pulls===0 && controller.desiredSize===0"),
        Value::Bool(true)
    );
    run(
        &mut vm,
        "const reader=stream.getReader();reader.read().then(r=>values.push(r.value));",
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "pulls===1 && values.join(',')==='initial' && controller.desiredSize===0"
        ),
        Value::Bool(true)
    );
}

#[test]
fn stream_async_start_blocks_pull_and_pending_reads_until_it_settles() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let started,pulls=0,result=null;
        const stream=new ReadableStream({start(){return new Promise(resolve=>started=resolve);},pull(c){pulls++;c.enqueue('later');c.close();}});
        const reader=stream.getReader();reader.read().then(r=>result=r.value);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "pulls===0 && result===null"),
        Value::Bool(true)
    );
    run(&mut vm, "started();");
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "pulls===1 && result==='later'"),
        Value::Bool(true)
    );
}

#[test]
fn stream_error_rejects_pending_read_and_closed_with_the_original_reason() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller,readError=null,closedError=null;
        const reason={message:'original'};
        const stream=new ReadableStream({start(c){controller=c;}},{highWaterMark:0});
        const reader=stream.getReader();
        reader.read().catch(e=>readError=e);reader.closed.catch(e=>closedError=e);
        controller.error(reason);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "readError===reason && closedError===reason && controller.desiredSize===null"
        ),
        Value::Bool(true)
    );
}

#[test]
fn stream_cancel_waits_for_source_promise_and_refuses_locked_stream_cancellation() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let finish,seen=null,settled=false,lockedError=false,readDone=false;
        const source={cancel(reason){seen=reason;return new Promise(resolve=>finish=resolve);}};
        const stream=new ReadableStream(source,{highWaterMark:0}),reader=stream.getReader();
        stream.cancel('foreign').catch(e=>lockedError=e instanceof TypeError);
        reader.read().then(r=>readDone=r.done);
        reader.cancel('owned').then(()=>settled=true);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "seen==='owned' && lockedError && readDone && !settled"
        ),
        Value::Bool(true)
    );
    run(&mut vm, "finish();");
    checkpoint(&mut vm);
    assert_eq!(run(&mut vm, "settled"), Value::Bool(true));
}

#[test]
fn releasing_a_reader_rejects_pending_reads_without_discarding_stream_data() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller,rejected=false,received=null;
        const stream=new ReadableStream({start(c){controller=c;}},{highWaterMark:0});
        const reader=stream.getReader();reader.read().catch(e=>rejected=e instanceof TypeError);
        reader.releaseLock();controller.enqueue('retained');
        stream.getReader().read().then(r=>received=r.value);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "rejected && received==='retained'"),
        Value::Bool(true)
    );
}

#[test]
fn blob_streams_deliver_real_byte_arrays_and_text_streams_decode_utf8() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        const blob=new Blob([new Uint8Array([0,255,65])]),stream=blob.stream();
        const reader=stream.getReader();let value=null,done=false,text=null;
        reader.read().then(r=>value=r.value);
        reader.read().then(r=>done=r.done);
        new Blob(['中文 🧊']).textStream().getReader().read().then(r=>text=r.value);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(run(&mut vm,"stream instanceof ReadableStream && value instanceof Uint8Array && Array.from(value).join(',')==='0,255,65' && done && text==='中文 🧊'"),Value::Bool(true));
}

#[test]
fn blob_byte_stream_byob_transfers_the_input_buffer_and_returns_exact_views() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        const stream=new Blob([new Uint8Array([0,255,65])]).stream();
        const reader=stream.getReader({mode:'byob'}),input=new Uint8Array(5);
        let value=null,last=null;
        reader.read(input).then(r=>value=r.value);
        reader.read(new Uint8Array(1)).then(r=>last=r);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(run(&mut vm,"reader instanceof ReadableStreamBYOBReader && input.buffer.byteLength===0 && value instanceof Uint8Array && Array.from(value).join(',')==='0,255,65' && last.done && last.value.byteLength===0"),Value::Bool(true));
}

#[test]
fn byte_stream_exposes_a_real_byob_request_and_responds_into_the_owned_view() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let request=null,result=null;
        const stream=new ReadableStream({type:'bytes',pull(controller){
            request=controller.byobRequest;
            request.view[0]=7;request.view[1]=255;request.respond(2);controller.close();
        }});
        const reader=stream.getReader({mode:'byob'});reader.read(new Uint8Array(4)).then(r=>result=r.value);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(run(&mut vm,"request instanceof ReadableStreamBYOBRequest && request.view===null && Array.from(result).join(',')==='7,255'"),Value::Bool(true));
}

#[test]
fn stream_strategy_errors_and_invalid_receivers_keep_native_brands() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        let rejected=0;
        for(const action of [()=>ReadableStream(),()=>new ReadableStream({}, {highWaterMark:-1}),
            ()=>new ReadableStream({type:'invalid'}),()=>new ReadableStream({type:'bytes'},{size(){return 1;}}),
            ()=>ReadableStream.prototype.getReader.call({}),()=>new ReadableStreamDefaultController()]) {
            try{action();}catch(e){if(e instanceof TypeError || e instanceof RangeError) rejected++;}
        }
        rejected===6
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn blob_text_stream_preserves_scalars_split_across_source_chunks() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        const expected='x'.repeat(65535)+'🧊中文';
        const reader=new Blob([expected]).textStream().getReader();let actual='',done=false;
        reader.read().then(r=>actual+=r.value);
        reader.read().then(r=>actual+=r.value);
        reader.read().then(r=>done=r.done);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(run(&mut vm, "actual===expected && done"), Value::Bool(true));
}

#[test]
fn async_pull_is_single_flight_and_a_later_read_retries_after_settlement() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let finish,controller,pulls=0,values=[];
        const stream=new ReadableStream({start(c){controller=c;},pull(){pulls++;return new Promise(resolve=>finish=resolve);}},{highWaterMark:0});
        const reader=stream.getReader();reader.read().then(r=>values.push(r.value));reader.read().then(r=>values.push(r.value));
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "pulls===1 && values.length===0"),
        Value::Bool(true)
    );
    run(&mut vm, "controller.enqueue('first');finish();");
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "pulls===2 && values.join(',')==='first'"),
        Value::Bool(true)
    );
    run(
        &mut vm,
        "controller.enqueue('second');controller.close();finish();",
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "values.join(',')==='first,second'"),
        Value::Bool(true)
    );
}

#[test]
fn byte_controller_close_leaves_a_pending_byob_request_for_zero_response() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller,request,result=null;
        const stream=new ReadableStream({type:'bytes',start(c){controller=c;}});
        stream.getReader({mode:'byob'}).read(new Uint8Array(3)).then(r=>result=r);
        request=controller.byobRequest;controller.close();
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "result===null && request.view!==null"),
        Value::Bool(true)
    );
    run(&mut vm, "request.respond(0);");
    checkpoint(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "result.done && result.value.byteLength===0 && request.view===null"
        ),
        Value::Bool(true)
    );
}

#[test]
fn stream_size_algorithm_errors_both_controller_and_reader_with_the_same_reason() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller,reason={message:'size'},thrown=null,rejected=null;
        const stream=new ReadableStream({start(c){controller=c;}},{highWaterMark:0,size(){throw reason;}});
        try{controller.enqueue('chunk');}catch(e){thrown=e;}
        stream.getReader().read().catch(e=>rejected=e);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "thrown===reason && rejected===reason && controller.desiredSize===null"
        ),
        Value::Bool(true)
    );
}

#[test]
fn stream_queue_and_callbacks_survive_collection_without_script_references() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let result=null,controller;
        (()=>{const stream=new ReadableStream({start(c){controller=c;}},{highWaterMark:0});
            stream.getReader().read().then(r=>result=r.value.marker);})();
        for(let i=0;i<100;i++) new Blob(['garbage '+i]);
        controller.enqueue({marker:'rooted'});
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(run(&mut vm, "result==='rooted'"), Value::Bool(true));
}

#[test]
fn byte_stream_minimum_read_waits_for_aligned_data_and_close_rejects_partial_elements() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller,result=null,error=null,closeError=null;
        const stream=new ReadableStream({type:'bytes',start(c){controller=c;}});
        const reader=stream.getReader({mode:'byob'});
        reader.read(new Uint16Array(2),{min:2}).then(r=>result=r.value,e=>error=e);
        controller.enqueue(new Uint8Array([1,2]));
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "result===null && error===null"),
        Value::Bool(true)
    );
    run(
        &mut vm,
        "controller.enqueue(new Uint8Array([3]));try{controller.close();}catch(e){closeError=e;}",
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "result===null && error instanceof TypeError && closeError===error"
        ),
        Value::Bool(true)
    );
}

#[test]
fn byob_enqueue_transfers_a_previously_exposed_request_buffer() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller,result=null;
        const stream=new ReadableStream({type:'bytes',start(c){controller=c;}});
        const reader=stream.getReader({mode:'byob'});reader.read(new Uint8Array(4)).then(r=>result=r);
        const request=controller.byobRequest,oldView=request.view;
        controller.enqueue(new Uint8Array([7,8]));
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(run(&mut vm,"oldView.buffer.byteLength===0 && request.view===null && !result.done && Array.from(result.value).join(',')==='7,8'"),Value::Bool(true));
}

#[test]
fn byob_cancel_resolves_pending_reads_with_undefined_and_invalidates_requests() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller,result=null;
        const stream=new ReadableStream({type:'bytes',start(c){controller=c;}}),reader=stream.getReader({mode:'byob'});
        reader.read(new Uint8Array(4)).then(r=>result=r);
        const request=controller.byobRequest;reader.cancel();
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(
            &mut vm,
            "result.done && result.value===undefined && request.view===null"
        ),
        Value::Bool(true)
    );
}

#[test]
fn byte_auto_allocation_does_not_split_an_enqueued_default_reader_chunk() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let controller,result=null;
        const stream=new ReadableStream({type:'bytes',autoAllocateChunkSize:2,start(c){controller=c;}});
        stream.getReader().read().then(r=>result=r.value);
        controller.enqueue(new Uint8Array([1,2,3,4]));
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(
        run(&mut vm, "Array.from(result).join(',')==='1,2,3,4'"),
        Value::Bool(true)
    );
}

#[test]
fn byob_respond_with_new_view_preserves_the_original_read_view_type() {
    let mut vm = web_vm();
    run(
        &mut vm,
        r#"
        let result=null;
        const stream=new ReadableStream({type:'bytes',pull(c){
            const request=c.byobRequest,view=request.view;
            view[0]=7;view[1]=8;request.respondWithNewView(new Uint8Array(view.buffer,view.byteOffset,2));c.close();
        }});
        stream.getReader({mode:'byob'}).read(new DataView(new ArrayBuffer(5))).then(r=>result=r.value);
    "#,
    );
    checkpoint(&mut vm);
    assert_eq!(run(&mut vm,"result instanceof DataView && result.byteLength===2 && result.getUint8(0)===7 && result.getUint8(1)===8"),Value::Bool(true));
}
