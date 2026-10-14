// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Behavioral reference: https://dom.spec.whatwg.org/#concept-event-dispatch
use blueice_bluejs::{
    compile, parse, HostFileSelectionEvent, HostFunctionError, HostObjectFamily, HostObjectKey,
    HostValue, Value, Vm, VmConfig,
};
use std::{cell::Cell, rc::Rc};

fn browser_vm() -> Vm {
    browser_parts().0
}
fn browser_parts() -> (Vm, HostObjectFamily, HostObjectKey) {
    browser_parts_with_path(|key: HostObjectKey| {
        Ok((1..=key.object())
            .rev()
            .map(|id| HostObjectKey::new(7, 4, id))
            .collect())
    })
}
fn browser_parts_with_path(
    path: impl FnMut(HostObjectKey) -> Result<Vec<HostObjectKey>, HostFunctionError> + 'static,
) -> (Vm, HostObjectFamily, HostObjectKey) {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    vm.install_host_node_event_methods(family, path).unwrap();
    for (name, id) in [("target", 3), ("parent", 2), ("doc", 1)] {
        vm.install_host_object_factory(name, 0, family, move |_: &[HostValue]| {
            Ok(Some(HostObjectKey::new(7, 4, id)))
        })
        .unwrap();
    }
    (vm, family, HostObjectKey::new(7, 4, 3))
}

#[test]
fn invalid_owner_paths_do_not_dispatch_or_leave_the_event_active() {
    let case = Rc::new(Cell::new(0));
    let selected = case.clone();
    let (mut vm, _, _) = browser_parts_with_path(move |key| match selected.get() {
        0 => Ok(vec![]),
        1 => Ok(vec![HostObjectKey::new(7, 4, 2)]),
        2 => Ok(vec![key, key]),
        3 => Ok(vec![key, HostObjectKey::new(8, 4, 1)]),
        4 => Ok(vec![key, HostObjectKey::new(7, 5, 1)]),
        5 => Ok((0..257)
            .map(|index| HostObjectKey::new(7, 4, 3 + index))
            .collect()),
        _ => Err(HostFunctionError::new("Document was replaced")),
    });
    vm.execute_script(
        &compile(
            &parse(
                "var event=new Event('x');var count=0;target().addEventListener('x',()=>count++);",
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    for index in 0..7 {
        case.set(index);
        assert_eq!(vm.execute_script(&compile(&parse("(()=>{let rejected=false;try{target().dispatchEvent(event);}catch(error){rejected=error instanceof TypeError;} return rejected && count===0 && event.eventPhase===0 && event.currentTarget===null && event.target===null;})()").unwrap()).unwrap()).unwrap(),Value::Bool(true));
    }
}

#[test]
fn dispatch_snapshots_ancestry_but_observes_listeners_added_before_their_phase() {
    let moved = Rc::new(Cell::new(false));
    let path_state = moved.clone();
    let (mut vm, family, _) = browser_parts_with_path(move |key| {
        Ok(vec![
            key,
            HostObjectKey::new(7, 4, if path_state.get() { 9 } else { 2 }),
            HostObjectKey::new(7, 4, 1),
        ])
    });
    vm.install_host_function("movePath", 0, move |_: &[HostValue]| {
        moved.set(true);
        Ok(HostValue::Undefined)
    })
    .unwrap();
    vm.install_host_object_factory("movedParent", 0, family, move |_: &[HostValue]| {
        Ok(Some(HostObjectKey::new(7, 4, 9)))
    })
    .unwrap();
    let source = r#"
        let calls=[];
        doc().addEventListener('x',()=>{calls.push('doc');movePath();parent().addEventListener('x',()=>calls.push('late'),{once:true});},true);
        parent().addEventListener('x',()=>calls.push('original'));
        movedParent().addEventListener('x',()=>calls.push('moved'));
        target().dispatchEvent(new Event('x',{bubbles:true}));
        target().dispatchEvent(new Event('x',{bubbles:true}));
        calls.join(',')==='doc,original,late,doc,moved'
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn dynamic_node_listener_names_are_charged_to_the_managed_heap_budget() {
    let mut vm = browser_vm();
    vm.execute_script(
        &compile(
            &parse("var node=target();var name='x'.repeat(32768);function callback(event){}")
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let before = vm.heap().stats().managed_bytes;
    vm.execute_script(&compile(&parse("node.addEventListener(name,callback)").unwrap()).unwrap())
        .unwrap();
    assert!(vm.heap().stats().managed_bytes >= before + 32000);
    vm.execute_script(
        &compile(&parse("node.removeEventListener(name,callback)").unwrap()).unwrap(),
    )
    .unwrap();
}

#[test]
fn native_node_clicks_are_trusted_branded_events_with_ancestry_and_cancellation() {
    let (mut vm, family, key) = browser_parts();
    let source = r#"
        let clicks=[], custom=0;
        target().addEventListener('custom',()=>custom++);
        doc().addEventListener('click',e=>clicks.push('doc:'+e.eventPhase+':'+e.isTrusted),true);
        parent().addEventListener('click',e=>clicks.push('parent:'+e.eventPhase));
        target().addEventListener('click',e=>{clicks.push('target:'+e.eventPhase+':'+(e instanceof Event));
            if(!e.bubbles || !e.composed || !e.cancelable || e.target!==target())throw 'attributes';e.preventDefault();});
    "#;
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    assert!(vm.dispatch_host_click(family, key).unwrap());
    assert_eq!(
        vm.execute_script(
            &compile(
                &parse("clicks.join(',')==='doc:1:true,target:2:true,parent:3' && custom===0")
                    .unwrap()
            )
            .unwrap()
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn native_selection_events_share_branded_base_attributes_and_keep_trust_separate() {
    let (mut vm, family, key) = browser_parts();
    vm.execute_script(&compile(&parse(r#"
        let observations=[];
        parent().addEventListener('input',e=>observations.push('parent:'+e.eventPhase));
        target().addEventListener('input',e=>observations.push([e instanceof Event,e.isTrusted,e.cancelable,e.composed,e.eventPhase].join(':')));
    "#).unwrap()).unwrap()).unwrap();
    vm.dispatch_host_file_selection_event(
        family,
        &[
            key,
            HostObjectKey::new(7, 4, 2),
            HostObjectKey::new(7, 4, 1),
        ],
        HostFileSelectionEvent::Input,
    )
    .unwrap();
    assert_eq!(
        vm.execute_script(
            &compile(&parse("observations.join(',')==='true:true:false:true:2,parent:3'").unwrap())
                .unwrap()
        )
        .unwrap(),
        Value::Bool(true)
    );
}

fn run(source: &str) -> Value {
    browser_vm()
        .execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn node_custom_events_follow_capture_target_and_bubble_with_live_wrapper_identity() {
    assert_eq!(
        run(r#"
        const t=target(), p=parent(), d=doc(), calls=[], payload={value:7};
        const event=new CustomEvent('data',{detail:payload,bubbles:true,cancelable:true});
        function observe(label,node){return function(e){const path=e.composedPath();
            if(e!==event || e.target!==t || e.currentTarget!==node || this!==node || e.detail!==payload
                || e.isTrusted || path.length!==3 || path[0]!==t || path[1]!==p || path[2]!==d) throw 'identity';
            calls.push(label+':'+e.eventPhase);};}
        d.addEventListener('data',observe('doc-c',d),true);
        p.addEventListener('data',observe('parent-c',p),true);
        t.addEventListener('data',observe('target-c',t),true);
        t.addEventListener('data',observe('target-b',t));
        p.addEventListener('data',observe('parent-b',p));
        d.addEventListener('data',observe('doc-b',d));
        const result=t.dispatchEvent(event);
        result && calls.join(',')==='doc-c:1,parent-c:1,target-c:2,target-b:2,parent-b:3,doc-b:3'
            && event.target===t && event.currentTarget===null && event.eventPhase===0 && event.composedPath().length===0
            && t instanceof EventTarget
    "#),
        Value::Bool(true)
    );
}

#[test]
fn nonbubbling_events_still_capture_and_eventtarget_borrowed_members_work_on_nodes() {
    assert_eq!(
        run(r#"
        const t=target(), calls=[];
        function capture(e){calls.push('capture:'+e.eventPhase);}
        function targetCall(e){calls.push('target:'+e.eventPhase);}
        EventTarget.prototype.addEventListener.call(parent(),'x',capture,true);
        EventTarget.prototype.addEventListener.call(t,'x',targetCall);
        doc().addEventListener('x',()=>calls.push('unexpected-bubble'));
        EventTarget.prototype.dispatchEvent.call(t,new Event('x'));
        EventTarget.prototype.removeEventListener.call(t,'x',targetCall);
        t.dispatchEvent(new Event('x'));
        calls.join(',')==='capture:1,target:2,capture:1'
    "#),
        Value::Bool(true)
    );
}

#[test]
fn node_listener_once_passive_signal_removal_and_throwing_callbacks_use_live_snapshots() {
    assert_eq!(
        run(r#"
        const t=target(), calls=[], controller=new AbortController();
        function removed(){calls.push('removed');}
        t.addEventListener('x',function first(e){calls.push('once');e.preventDefault();
            t.removeEventListener('x',removed);controller.abort();throw 'page error';},{once:true,passive:true});
        t.addEventListener('x',removed);
        t.addEventListener('x',()=>calls.push('aborted'),{signal:controller.signal});
        const handler={handleEvent(e){calls.push(this===handler?'object':'bad');e.preventDefault();}};
        t.addEventListener('x',handler);
        const a=new Event('x',{cancelable:true}), b=new Event('x',{cancelable:true});
        !t.dispatchEvent(a) && !t.dispatchEvent(b) && calls.join(',')==='once,object,object'
            && a.defaultPrevented && b.defaultPrevented
    "#),
        Value::Bool(true)
    );
}

#[test]
fn node_dispatch_reentrancy_and_stop_flags_do_not_corrupt_successor_events() {
    assert_eq!(
        run(r#"
        const t=target(), calls=[];let nested=false;
        const event=new Event('x',{bubbles:true});
        t.addEventListener('x',function first(e){calls.push('first');
            try{t.dispatchEvent(e);}catch(error){calls.push(error.name);}
            if(!nested){nested=true;t.dispatchEvent(new CustomEvent('nested',{detail:'child'}));}
            e.stopImmediatePropagation();});
        t.addEventListener('nested',e=>calls.push(e.detail));
        t.addEventListener('x',()=>calls.push('blocked'));
        parent().addEventListener('x',()=>calls.push('parent'));
        t.dispatchEvent(event);t.dispatchEvent(event);
        calls.join(',')==='first,InvalidStateError,child,first,InvalidStateError'
            && event.eventPhase===0 && !event.cancelBubble && event.currentTarget===null
    "#),
        Value::Bool(true)
    );
}

#[test]
fn synthetic_native_named_events_remain_untrusted_and_reject_forged_receivers() {
    assert_eq!(
        run(r#"
        const t=target();let seen='';
        t.addEventListener('change',e=>{seen=e.type+':'+e.isTrusted+':'+(e instanceof Event);});
        t.dispatchEvent(new Event('change',{bubbles:true}));
        let rejected=0;for(const action of [()=>t.dispatchEvent({type:'change'}),
            ()=>t.dispatchEvent.call(Object.create(t),new Event('change')),
            ()=>t.dispatchEvent.call({},new Event('change'))]){
            try{action();}catch(error){if(error instanceof TypeError)rejected++;}
        }
        seen==='change:false:true' && rejected===3
    "#),
        Value::Bool(true)
    );
}
