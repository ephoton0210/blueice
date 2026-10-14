// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Behavioral reference: https://dom.spec.whatwg.org/#interface-customevent
use blueice_bluejs::{compile, parse, HeapConfig, Value, Vm, VmConfig};

fn web_vm() -> Vm {
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
    vm
}

fn run(source: &str) -> Value {
    web_vm()
        .execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn custom_event_has_event_inheritance_and_exact_retained_detail() {
    assert_eq!(
        run(r#"
        const payload={value:7}, event=new CustomEvent('data', {detail:payload,cancelable:true});
        const initial=event.detail===payload;
        event.detail='replacement';
        for(let i=0;i<80;i++){new EventTarget();}
        initial && event.detail===payload && event.detail.value===7 && event instanceof CustomEvent
            && event instanceof Event && !(event instanceof UIEvent) && !event.isTrusted
            && Object.getPrototypeOf(CustomEvent)===Event
            && Object.getPrototypeOf(CustomEvent.prototype)===Event.prototype
            && Object.prototype.toString.call(event)==='[object CustomEvent]'
            && new CustomEvent('x').detail===null && new CustomEvent('x',{detail:undefined}).detail===null
    "#),
        Value::Bool(true)
    );
}

#[test]
fn custom_event_dictionary_order_and_arbitrary_detail_do_not_coerce_payload() {
    assert_eq!(
        run(r#"
        const calls=[], marker=Symbol('detail');
        const event=new CustomEvent({toString(){calls.push('type');return 'data';}}, {
            get bubbles(){calls.push('bubbles');return true;},
            get cancelable(){calls.push('cancelable');return true;},
            get composed(){calls.push('composed');return true;},
            get detail(){calls.push('detail');return marker;}
        });
        event.detail===marker && event.bubbles && event.cancelable && event.composed
            && calls.join(',')==='type,bubbles,cancelable,composed,detail'
            && new CustomEvent('x',{detail:3n}).detail===3n
            && new CustomEvent('x',{detail:null}).detail===null
    "#),
        Value::Bool(true)
    );
}

#[test]
fn custom_event_dispatch_preserves_payload_and_legacy_init_cannot_mutate_active_event() {
    assert_eq!(
        run(r#"
        const target=new EventTarget(), payload={message:'retained'}, event=new CustomEvent('data',{detail:payload,cancelable:true});
        let observed=false;
        target.addEventListener('data',function(e){
            e.initCustomEvent('replacement',false,false,'forged');
            observed=e===event && e.detail===payload && e.type==='data' && e.target===target && !e.isTrusted;
            e.preventDefault();
        });
        const result=target.dispatchEvent(event);
        event.initCustomEvent('next',true,false,payload);
        const reset=event.type==='next' && event.bubbles && !event.cancelable && !event.defaultPrevented
            && event.target===null && event.detail===payload;
        event.initEvent('base');
        const baseKept=event.detail===payload && event.type==='base';
        event.initCustomEvent('default');
        observed && !result && reset && baseKept && event.detail===null
    "#),
        Value::Bool(true)
    );
}

#[test]
fn custom_event_native_members_reject_sibling_event_brands() {
    assert_eq!(
        run(r#"
        const event=new CustomEvent('x');let rejected=0;
        const detail=Object.getOwnPropertyDescriptor(CustomEvent.prototype,'detail');
        for(const action of [()=>CustomEvent('x'),()=>new CustomEvent(),()=>detail.get.call(new Event('x')),
            ()=>detail.get.call(new UIEvent('x')),()=>CustomEvent.prototype.initCustomEvent.call(new ProgressEvent('x'),'y'),
            ()=>Object.getOwnPropertyDescriptor(UIEvent.prototype,'detail').get.call(event),
            ()=>MouseEvent.prototype.getModifierState.call(event,'Shift'),
            ()=>UIEvent.prototype.initUIEvent.call(event,'x')]) {
            try{action();}catch(error){if(error instanceof TypeError)rejected++;}
        }
        rejected===8 && detail.enumerable && detail.configurable && detail.set===undefined
            && CustomEvent.prototype.initCustomEvent.length===1 && CustomEvent.length===1
    "#),
        Value::Bool(true)
    );
}

#[test]
fn custom_event_subclasses_and_reentrant_legacy_conversion_preserve_live_state() {
    assert_eq!(
        run(r#"
        class Child extends CustomEvent {}
        const event=new Child('before',{composed:true});
        const marker={value:'latest'};
        event.initCustomEvent({toString(){event.initCustomEvent('nested',false,true,{old:true});
            for(let i=0;i<40;i++){new EventTarget();} return 'outer';}},true,false,marker);
        for(let i=0;i<40;i++){new EventTarget();}
        event instanceof Child && event instanceof CustomEvent && event instanceof Event
            && event.type==='outer' && event.detail===marker && event.detail.value==='latest'
            && event.bubbles && !event.cancelable && event.composed && !event.isTrusted
    "#),
        Value::Bool(true)
    );
}
