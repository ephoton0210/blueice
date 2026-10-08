// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::HostFileSelectionEvent;
use blueice_bluejs::{
    compile, parse, HeapConfig, HostFileData, HostFunctionError, HostInputFilesUpdate,
    HostObjectKey, RuntimeError, Value, Vm, VmConfig,
};
use std::{cell::Cell, rc::Rc};

fn run(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
}

fn web_vm() -> Vm {
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    vm
}

#[test]
fn blob_and_file_are_branded_and_normalize_metadata() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        const blob = new Blob(['a', '中文', '\ud800'], {type:'TEXT/PLAIN'});
        const file = new File([blob], 'chosen/中文.bin', {lastModified:123, type:'X/BINARY'});
        let branded = false;
        try { Object.getOwnPropertyDescriptor(Blob.prototype,'size').get.call({}); }
        catch(error) { branded = error instanceof TypeError; }
        let needsNew = false;
        try { Blob([]); } catch(error) { needsNew = error instanceof TypeError; }
        branded && needsNew && blob.size === 10 && blob.type === 'text/plain' &&
        file instanceof Blob && file instanceof File && file.name === 'chosen/中文.bin' &&
        file.size === 10 && file.type === 'x/binary' && file.lastModified === 123 &&
        Object.prototype.toString.call(blob) === '[object Blob]' &&
        Object.prototype.toString.call(file) === '[object File]'
    "#
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn blob_snapshots_binary_parts_slices_and_reads_through_real_promises() {
    let mut vm = web_vm();
    assert_eq!(run(&mut vm, r#"
        const data = new Uint8Array([0,255,13,10,7]);
        const view = new DataView(data.buffer,1,3);
        const blob = new Blob([view, data.subarray(4), new Blob(['z'])]);
        data.fill(22);
        const sliced = blob.slice(-3,99,'APP/BINARY');
        let binary = '';
        let text = '';
        let copied = '';
        const promise = blob.bytes();
        promise.then(function(bytes) { binary = Array.from(bytes).join(','); bytes.fill(8); });
        blob.arrayBuffer().then(function(buffer) { copied = Array.from(new Uint8Array(buffer)).join(','); });
        blob.text().then(function(value) { text = value; });
        promise instanceof Promise && sliced.size === 3 && sliced.type === 'app/binary'
    "#).unwrap(), Value::Bool(true));
    assert_eq!(
        run(&mut vm, "binary === '' && copied === '' && text === ''").unwrap(),
        Value::Bool(true)
    );
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r"binary === '255,13,10,7,122' && copied === binary && text === '\ufffd\r\n\u0007z'"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        run(
            &mut vm,
            "sliced.bytes().then(function(bytes) { binary=Array.from(bytes).join(','); }); true"
        )
        .unwrap(),
        Value::Bool(true)
    );
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        run(&mut vm, "binary === '10,7,122'").unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn blob_text_strips_only_the_leading_utf8_bom_without_changing_binary_content() {
    let mut vm = web_vm();
    run(&mut vm, r#"
        const bomBlob = new Blob([new Uint8Array([239,187,191,97,239,187,191,0,255])], {type:'text/plain;charset=utf-16'});
        let bomText = '';
        let bomBytes = '';
        bomBlob.text().then(function(value) { bomText = value; });
        bomBlob.bytes().then(function(value) { bomBytes = Array.from(value).join(','); });
    "#).unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        run(
            &mut vm,
            r"bomText === 'a\ufeff\u0000\ufffd' && bomBytes === '239,187,191,97,239,187,191,0,255'"
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn construction_converts_options_before_copying_views_and_only_normalizes_strings() {
    let mut vm = web_vm();
    assert_eq!(
        run(
            &mut vm,
            r#"
        let order='';
        const bytes=new Uint8Array([13,10]);
        const part={toString:function(){order+='s';return '\r\n';}};
        const name={toString:function(){order+='n';return 'chosen';}};
        const options={
            get endings(){order+='e';return 'native';},
            get lastModified(){order+='l';bytes[0]=7;return -1;},
            get type(){order+='t';return 'TEXT/PLAIN';}
        };
        const result=new File([part,bytes],name,options);
        let content='';
        result.bytes().then(function(value){content=Array.from(value).join(',');});
        order==='snelt' && result.lastModified===-1 && result.type==='text/plain'
    "#
        )
        .unwrap(),
        Value::Bool(true)
    );
    vm.run_promise_jobs().unwrap();
    let expected = if cfg!(windows) {
        "content==='13,10,7,10'"
    } else {
        "content==='10,7,10'"
    };
    assert_eq!(run(&mut vm, expected).unwrap(), Value::Bool(true));
}

#[test]
fn invalid_metadata_arguments_and_part_iteration_preserve_exceptions() {
    let mut vm = web_vm();
    assert_eq!(run(&mut vm, r#"
        let closed=false;
        const marker={};
        const iterable={
          [Symbol.iterator]:function(){return {
            next:function(){return {value:{toString:function(){throw marker;}},done:false};},
            return:function(){closed=true;return {};}
          };}
        };
        let original=false;
        try{new Blob(iterable);}catch(error){original=error===marker;}
        let invalid=0;
        for(const call of [function(){return new Blob('x');},function(){return new Blob([],7);},
          function(){return new Blob([],{endings:'invalid'});},function(){return new File([]);},
          function(){return Object.getOwnPropertyDescriptor(File.prototype,'name').get.call(new Blob());}]){
          try{call();}catch(error){if(error instanceof TypeError)invalid++;}
        }
        original && closed && invalid===5 && new Blob([],{type:'a\u0080b'}).type==='' &&
        new Blob().size===0 && new Blob(['abcdef']).slice(Infinity).size===0 &&
        new Blob(['abcdef']).slice(-Infinity,Infinity).size===6
    "#).unwrap(), Value::Bool(true));
}

#[test]
fn file_lists_preserve_identity_snapshots_iteration_and_reject_forged_receivers() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("input", 0, family, |_: &[blueice_bluejs::HostValue]| {
        Ok(Some(HostObjectKey::new(7, 2, 11)))
    })
    .unwrap();
    let revision = Rc::new(Cell::new(1));
    let current = Rc::clone(&revision);
    vm.install_host_file_input_reader(family, move |key: HostObjectKey, known: Option<u64>| {
        if !key.matches_owner(7, 2) {
            return Err(HostFunctionError::new("stale document"));
        }
        if known == Some(current.get()) {
            return Ok(HostInputFilesUpdate::Unchanged);
        }
        Ok(HostInputFilesUpdate::Selected {
            revision: current.get(),
            files: if current.get() == 1 {
                vec![HostFileData {
                    name: "chosen-中文.bin".into(),
                    media_type: "application/octet-stream".into(),
                    last_modified: 123,
                    bytes: vec![0, 255, 13, 10, 7],
                }]
            } else {
                vec![]
            },
        })
    })
    .unwrap();
    assert_eq!(
        run(
            &mut vm,
            r#"
      const node=input(); const first=node.files; const file=first[0];
      let forged=false;
      try{Object.getOwnPropertyDescriptor(Object.getPrototypeOf(node),'files').get.call({});}
      catch(error){forged=error instanceof TypeError;}
      let wrongList=false;
      try{FileList.prototype.item.call([],0);}catch(error){wrongList=error instanceof TypeError;}
      forged && wrongList && first===node.files && first instanceof FileList && first.length===1 &&
      first.item(0)===file && first.item(1)===null && first.item(-1)===null &&
      Array.from(first)[0]===file && file instanceof File && file instanceof Blob &&
      file.name==='chosen-中文.bin' && file.lastModified===123 && file.size===5
    "#
        )
        .unwrap(),
        Value::Bool(true)
    );
    revision.set(2);
    assert_eq!(
        run(
            &mut vm,
            "Object.defineProperty(first,'length',{value:0});Array.from(first)[0]===file"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        run(
            &mut vm,
            "node.files.length===0 && first.item(0)===file && first[0]===file"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(run(&mut vm,"let exact='';file.bytes().then(function(bytes){exact=Array.from(bytes).join(',');});true").unwrap(),Value::Bool(true));
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        run(&mut vm, "exact==='0,255,13,10,7'").unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn selection_events_bubble_in_order_and_keep_callbacks_and_retained_events_alive() {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_family_object("document", family, HostObjectKey::new(7, 2, 2))
        .unwrap();
    vm.install_host_object_factory(
        "node",
        1,
        family,
        |arguments: &[blueice_bluejs::HostValue]| {
            let [blueice_bluejs::HostValue::Number(number)] = arguments else {
                return Err(HostFunctionError::new("node index"));
            };
            Ok(Some(HostObjectKey::new(7, 2, *number as u64)))
        },
    )
    .unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    assert_eq!(run(&mut vm,r#"
      const input=node(1), parent=document; let report=[];let retained=null;
      const listener=function(event){
        const correct=event.target===input && event.currentTarget===this && event.bubbles && !event.cancelable && event.isTrusted;
        report.push(event.type+':'+(this===input?'input':'parent')+':'+correct+':'+event.composed);
        event.preventDefault();retained=event;
      };
      for(const type of ['input','change','cancel']) {
        input.addEventListener(type,listener);input.addEventListener(type,listener);parent.addEventListener(type,listener);
      }
      true
    "#).unwrap(),Value::Bool(true));
    let path = [HostObjectKey::new(7, 2, 1), HostObjectKey::new(7, 2, 2)];
    for kind in [
        HostFileSelectionEvent::Input,
        HostFileSelectionEvent::Change,
        HostFileSelectionEvent::Cancel,
    ] {
        vm.dispatch_host_file_selection_event(family, &path, kind)
            .unwrap();
    }
    assert_eq!(run(&mut vm,r#"report.join(',')==='input:input:true:true,input:parent:true:true,change:input:true:false,change:parent:true:false,cancel:input:true:false,cancel:parent:true:false' && retained.currentTarget===null && !retained.defaultPrevented"#).unwrap(),Value::Bool(true));
    assert!(vm
        .dispatch_host_file_selection_event(
            family,
            &[path[0], HostObjectKey::new(8, 2, 2)],
            HostFileSelectionEvent::Input
        )
        .is_err());
    assert_eq!(
        run(
            &mut vm,
            "input.removeEventListener('change',listener);report=[];true"
        )
        .unwrap(),
        Value::Bool(true)
    );
    vm.dispatch_host_file_selection_event(family, &path, HostFileSelectionEvent::Change)
        .unwrap();
    assert_eq!(
        run(&mut vm, "report.join(',')==='change:parent:true:false'").unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn stop_immediate_propagation_and_throwing_listeners_do_not_corrupt_successor_events() {
    let mut vm = Vm::default();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("node", 1, family, |args: &[blueice_bluejs::HostValue]| {
        let [blueice_bluejs::HostValue::Number(number)] = args else {
            return Err(HostFunctionError::new("index"));
        };
        Ok(Some(HostObjectKey::new(7, 2, *number as u64)))
    })
    .unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    assert_eq!(run(&mut vm,r#"
      const input=node(1),parent=node(2);let report='';
      input.addEventListener('input',function(event){report+='a';event.stopImmediatePropagation();throw 'page failure';});
      input.addEventListener('input',function(){report+='b';});parent.addEventListener('input',function(){report+='c';});
      input.addEventListener('change',function(){report+='d';});parent.addEventListener('change',function(){report+='e';});true
    "#).unwrap(),Value::Bool(true));
    let path = [HostObjectKey::new(7, 2, 1), HostObjectKey::new(7, 2, 2)];
    vm.dispatch_host_file_selection_event(family, &path, HostFileSelectionEvent::Input)
        .unwrap();
    vm.dispatch_host_file_selection_event(family, &path, HostFileSelectionEvent::Change)
        .unwrap();
    assert_eq!(run(&mut vm, "report==='ade'").unwrap(), Value::Bool(true));
}

#[test]
fn file_api_is_owner_installed_and_survives_collection_with_accounted_bytes() {
    assert_eq!(
        run(
            &mut Vm::default(),
            "typeof Blob === 'undefined' && typeof File === 'undefined'"
        )
        .unwrap(),
        Value::Bool(true)
    );
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 2,
            major_threshold_bytes: 4096,
            max_heap_bytes: 4 * 1024 * 1024,
        },
        ..VmConfig::default()
    })
    .unwrap();
    vm.install_web_file_api().unwrap();
    let before = vm.heap().stats().managed_bytes;
    assert_eq!(run(&mut vm, "const retained=new File([new Uint8Array(32768)],'x'); for(let i=0;i<100;i++){new Blob(['temporary']);} retained.size===32768").unwrap(), Value::Bool(true));
    assert!(vm.heap().stats().managed_bytes >= before + 32768);
    assert_eq!(
        run(
            &mut vm,
            "retained.name==='x' && retained instanceof Blob && retained instanceof File"
        )
        .unwrap(),
        Value::Bool(true)
    );
}
