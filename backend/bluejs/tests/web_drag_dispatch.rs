// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, HostDragEventData, HostDragEventKind, HostDragItem, HostFileData,
    HostObjectFamily, HostObjectKey, HostValue, Value, Vm, VmConfig,
};

fn run(vm: &mut Vm, source: &str) -> Value {
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

fn browser_vm() -> (Vm, HostObjectFamily, HostObjectKey) {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_element_validator(family, |_| true).unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    let key = HostObjectKey::new(7, 2, 11);
    vm.install_host_object_factory("dragTarget", 0, family, move |_: &[HostValue]| {
        Ok(Some(key))
    })
    .unwrap();
    (vm, family, key)
}

#[test]
fn drag_image_export_keeps_owned_identity_gc_and_readwrite_scope() {
    let (mut vm, family, key) = browser_vm();
    run(
        &mut vm,
        r#"
        let retained;
        dragTarget().addEventListener('dragstart', function(event) {
            retained = event.dataTransfer;
            event.dataTransfer.setDragImage(dragTarget(), 4294967300.9, -3.1);
            const garbage = []; for (let i=0;i<80;i++) garbage.push({value:i});
        });
        dragTarget().addEventListener('dragover', function(event) {
            event.dataTransfer.setDragImage(dragTarget(), 8, 9);
        });
    "#,
    );
    let input = HostDragEventData::new(HostDragEventKind::DragStart, 1.0, 2.0);
    let result = vm.dispatch_host_drag_event(family, &[key], &input).unwrap();
    let image = result.image.unwrap();
    assert_eq!(image.element, key);
    assert_eq!((image.x, image.y), (4, -3));
    assert_eq!(
        run(
            &mut vm,
            "retained.setDragImage(dragTarget(), 5, 6) === undefined"
        ),
        Value::Bool(true)
    );
    let input = HostDragEventData::new(HostDragEventKind::DragOver, 1.0, 2.0);
    assert!(vm
        .dispatch_host_drag_event(family, &[key], &input)
        .unwrap()
        .image
        .is_none());
}

#[test]
fn owner_drag_related_target_is_branded_rooted_and_document_scoped() {
    let (mut vm, family, key) = browser_vm();
    let related = HostObjectKey::new(7, 2, 19);
    vm.install_host_object_factory("nextTarget", 0, family, move |_: &[HostValue]| {
        Ok(Some(related))
    })
    .unwrap();
    run(
        &mut vm,
        r#"
        const next = nextTarget();
        let retained, same = false;
        dragTarget().addEventListener('dragleave', function(event) {
            retained = event;
            same = event.relatedTarget === next && event.target === dragTarget();
            const allocations = [];
            for (let i = 0; i < 80; i++) allocations.push({value:i});
        });
    "#,
    );
    let mut input = HostDragEventData::new(HostDragEventKind::DragLeave, 1.0, 2.0);
    for foreign in [HostObjectKey::new(8, 2, 19), HostObjectKey::new(7, 3, 19)] {
        input.related_target = Some(foreign);
        assert!(vm.dispatch_host_drag_event(family, &[key], &input).is_err());
        assert_eq!(run(&mut vm, "retained === undefined"), Value::Bool(true));
    }
    input.related_target = Some(related);
    vm.dispatch_host_drag_event(family, &[key], &input).unwrap();
    assert_eq!(
        run(&mut vm, "same && retained.relatedTarget === next"),
        Value::Bool(true)
    );
    input.related_target = None;
    vm.dispatch_host_drag_event(family, &[key], &input).unwrap();
    assert_eq!(
        run(&mut vm, "retained.relatedTarget === null"),
        Value::Bool(true)
    );
}

#[test]
fn owner_drag_geometry_uses_document_and_padding_origins_during_dispatch() {
    let (mut vm, family, key) = browser_vm();
    run(
        &mut vm,
        r#"
        let retained, geometry;
        dragTarget().addEventListener('dragover', function(event) {
            retained = event;
            geometry = [event.clientX,event.clientY,event.pageX,event.pageY,event.offsetX,event.offsetY].join(',');
        });
    "#,
    );
    let mut input = HostDragEventData::new(HostDragEventKind::DragOver, 11.25, 22.5);
    input.page_x = 11.25;
    input.page_y = 222.5;
    input.offset_x = -3.75;
    input.offset_y = 12.5;
    input.page_y = f64::NAN;
    assert!(vm.dispatch_host_drag_event(family, &[key], &input).is_err());
    assert_eq!(run(&mut vm, "retained === undefined"), Value::Bool(true));
    input.page_y = 222.5;
    vm.dispatch_host_drag_event(family, &[key], &input).unwrap();
    assert_eq!(
        run(&mut vm, "geometry === '11.25,22.5,11.25,222.5,-3.75,12.5'"),
        Value::Bool(true)
    );
    // An event with no associated Window falls back to client coordinates
    // outside dispatch; offsets fall back to page coordinates.
    assert_eq!(run(&mut vm, "retained.offsetX === retained.pageX && retained.offsetY === retained.pageY && retained.pageY === retained.clientY"), Value::Bool(true));
}

#[test]
fn drag_listener_options_order_capture_once_passive_and_object_callbacks() {
    let (mut vm, family, key) = browser_vm();
    let parent = HostObjectKey::new(7, 2, 12);
    vm.install_host_object_factory("dragParent", 0, family, move |_: &[HostValue]| {
        Ok(Some(parent))
    })
    .unwrap();
    run(
        &mut vm,
        r#"
        const target = dragTarget(), parent = dragParent();
        const calls = []; let validPath = true, retainedEvent;
        const objectListener = {handleEvent(event) {
            calls.push('object:' + (this === objectListener) + ':' + event.eventPhase);
            event.preventDefault();
        }};
        parent.addEventListener('dragover', event => calls.push('capture:' + event.eventPhase), true);
        parent.addEventListener('dragover', event => calls.push('bubble:' + event.eventPhase));
        target.addEventListener('dragover', objectListener, {once:true, passive:true});
        target.addEventListener('dragover', event => {
            retainedEvent = event;
            calls.push('target:' + event.eventPhase + ':' + event.defaultPrevented);
            validPath = validPath && event instanceof MouseEvent && event.composedPath()[0] === target
                && event.composedPath()[1] === parent;
        });
    "#,
    );
    let event = HostDragEventData::new(HostDragEventKind::DragOver, 1.25, 2.5);
    assert!(
        !vm.dispatch_host_drag_event(family, &[key, parent], &event)
            .unwrap()
            .default_prevented
    );
    assert!(
        !vm.dispatch_host_drag_event(family, &[key, parent], &event)
            .unwrap()
            .default_prevented
    );
    assert_eq!(run(&mut vm, "validPath && retainedEvent.eventPhase === Event.NONE && retainedEvent.currentTarget === null && retainedEvent.composedPath().length === 0 && calls.join(',') === 'capture:1,object:true:2,target:2:false,bubble:3,capture:1,target:2:false,bubble:3'"), Value::Bool(true));
}

#[test]
fn drag_listener_capture_identity_removal_and_once_reentrancy_preserve_snapshots() {
    let (mut vm, family, key) = browser_vm();
    run(
        &mut vm,
        r#"
        const target = dragTarget();
        let captures = 0, bubbles = 0, once = 0, successor = 0;
        function listener(event) { if (event.eventPhase !== Event.AT_TARGET) throw new Error('phase');
            if (event.cancelBubble) bubbles++; else { captures++; event.cancelBubble = true; }
        }
        target.addEventListener('drop', listener, true);
        target.addEventListener('drop', listener, false);
        target.removeEventListener('drop', listener, false);
        target.addEventListener('drop', listener, false);
        function next(event) { successor++; }
        target.addEventListener('drop', function first(event) {
            once++; target.addEventListener('drop', next); event.preventDefault();
        }, {once:true});
    "#,
    );
    let event = HostDragEventData::new(HostDragEventKind::Drop, 1.0, 2.0);
    assert!(
        vm.dispatch_host_drag_event(family, &[key], &event)
            .unwrap()
            .default_prevented
    );
    assert!(
        !vm.dispatch_host_drag_event(family, &[key], &event)
            .unwrap()
            .default_prevented
    );
    assert_eq!(
        run(
            &mut vm,
            "captures === 2 && bubbles === 2 && once === 1 && successor === 1"
        ),
        Value::Bool(true)
    );
}

#[test]
fn drag_listener_option_getters_and_handle_event_lookup_run_at_the_correct_time() {
    let (mut vm, family, key) = browser_vm();
    run(
        &mut vm,
        r#"
        const target = dragTarget(); let getters = '', calls = 0, lookups = 0;
        const listener = {get handleEvent() { lookups++; return function(event) {calls++;}; }};
        const options = {
            get capture() {getters += 'c'; return false;},
            get once() {getters += 'o'; return false;},
            get passive() {getters += 'p'; return false;},
            get signal() {getters += 's'; return undefined;}
        };
        target.addEventListener('drag', listener, options);
        target.removeEventListener('drag', listener, options);
        target.addEventListener('drag', listener);
        target.addEventListener('drag', null);
        target.removeEventListener('drag', undefined);
    "#,
    );
    assert_eq!(
        run(&mut vm, "getters === 'copsc' && lookups === 0"),
        Value::Bool(true)
    );
    let event = HostDragEventData::new(HostDragEventKind::Drag, 1.0, 2.0);
    vm.dispatch_host_drag_event(family, &[key], &event).unwrap();
    assert_eq!(
        run(&mut vm, "lookups === 1 && calls === 1"),
        Value::Bool(true)
    );
}

#[test]
fn removed_and_readded_drag_callbacks_do_not_rejoin_the_current_dispatch_snapshot() {
    let (mut vm, family, key) = browser_vm();
    run(
        &mut vm,
        r#"
        const target = dragTarget(); let successors = 0;
        function successor(event) { successors++; }
        target.addEventListener('drop', function(event) {
            target.removeEventListener('drop', successor);
            target.addEventListener('drop', successor);
        }, {once:true});
        target.addEventListener('drop', successor);
    "#,
    );
    let event = HostDragEventData::new(HostDragEventKind::Drop, 0.0, 0.0);
    vm.dispatch_host_drag_event(family, &[key], &event).unwrap();
    assert_eq!(run(&mut vm, "successors === 0"), Value::Bool(true));
    vm.dispatch_host_drag_event(family, &[key], &event).unwrap();
    assert_eq!(run(&mut vm, "successors === 1"), Value::Bool(true));
}

#[test]
fn owner_drag_mouse_state_preserves_screen_fractions_buttons_and_modifier_changes() {
    let (mut vm, family, key) = browser_vm();
    run(
        &mut vm,
        r#"
        let states = [];
        dragTarget().addEventListener('dragover', event => {
            states.push([event.clientX,event.clientY,event.screenX,event.screenY,event.buttons,
                event.altKey,event.ctrlKey,event.metaKey,event.shiftKey,event.getModifierState('CapsLock')].join(':'));
        });
    "#,
    );
    let mut event = HostDragEventData::new(HostDragEventKind::DragOver, 1.25, -2.5);
    event.screen_x = -800.75;
    event.screen_y = 1024.125;
    event.buttons = 1;
    event.alt_key = true;
    event.ctrl_key = true;
    event.caps_lock = true;
    vm.dispatch_host_drag_event(family, &[key], &event).unwrap();
    event.alt_key = false;
    event.ctrl_key = false;
    event.meta_key = true;
    event.shift_key = true;
    event.buttons = 0;
    vm.dispatch_host_drag_event(family, &[key], &event).unwrap();
    assert_eq!(run(&mut vm,"states.join(',') === '1.25:-2.5:-800.75:1024.125:1:true:true:false:false:true,1.25:-2.5:-800.75:1024.125:0:false:false:true:true:true'"), Value::Bool(true));
}

#[test]
fn legacy_initializers_convert_arguments_but_do_not_reinitialize_an_active_dispatch() {
    let (mut vm, family, key) = browser_vm();
    run(
        &mut vm,
        r#"
        let retained, unchanged = false, conversions = 0;
        const target = dragTarget();
        target.addEventListener('dragover', event => {
            retained = event; event.preventDefault();
            const name = {toString() { conversions++; return 'mutated'; }};
            event.initUIEvent(name, false, false, null, 12);
            event.initMouseEvent(name, false, false, null, 3, 8, 9, 10, 11);
            unchanged = event.type === 'dragover' && event.clientX === 1.25
                && event.clientY === 2.5 && event.detail === 0 && event.isTrusted
                && event.defaultPrevented && event.currentTarget === target
                && event.eventPhase === Event.AT_TARGET;
        });
    "#,
    );
    let event = HostDragEventData::new(HostDragEventKind::DragOver, 1.25, 2.5);
    assert!(
        vm.dispatch_host_drag_event(family, &[key], &event)
            .unwrap()
            .default_prevented
    );
    assert_eq!(
        run(
            &mut vm,
            "unchanged && conversions === 2 && retained.type === 'dragover' && retained.isTrusted"
        ),
        Value::Bool(true)
    );
    assert_eq!(
        run(&mut vm, "retained.initUIEvent('after'); retained.type === 'after' && !retained.isTrusted && !retained.defaultPrevented && retained.target === null"),
        Value::Bool(true)
    );
}

#[test]
fn owner_drag_events_protect_content_then_disable_retained_stores_after_drop() {
    let (mut vm, family, key) = browser_vm();
    run(
        &mut vm,
        r#"
        const target = dragTarget();
        let entered = false, dropped = false, retained, itemList, fileList, retainedFile;
        target.addEventListener('dragenter', function(event) {
            retained = event.dataTransfer; itemList = retained.items; fileList = retained.files;
            entered = event.isTrusted && event.currentTarget === target && event.target === target
                && event.clientX === 18 && event.clientY === 27
                && retained.types.join(',') === 'text/plain,Files'
                && itemList.length === 2 && itemList[1].kind === 'file'
                && itemList[1].type === '' && fileList.length === 0
                && retained.getData('text') === '' && itemList[1].getAsFile() === null;
            retained.setData('text/plain', 'Forbidden');
            event.preventDefault();
        });
        target.addEventListener('drop', function(event) {
            retained = event.dataTransfer; itemList = retained.items; fileList = retained.files;
            retainedFile = fileList[0];
            retained.setData('text/plain', 'Forbidden');
            dropped = event.isTrusted && retained.getData('text') === 'Explicit 中文'
                && fileList.length === 1 && retainedFile.name === 'owned.bin';
            event.preventDefault();
        });
    "#,
    );
    let mut enter = HostDragEventData::new(HostDragEventKind::DragEnter, 18.0, 27.0);
    enter.types = vec!["text/plain".into(), "Files".into()];
    assert!(
        vm.dispatch_host_drag_event(family, &[key], &enter)
            .unwrap()
            .default_prevented
    );
    assert_eq!(run(&mut vm, "entered && retained.types.length === 0 && itemList.length === 0 && fileList.length === 0"), Value::Bool(true));
    let mut drop = HostDragEventData::new(HostDragEventKind::Drop, 18.0, 27.0);
    drop.items = vec![
        HostDragItem::Text {
            format: "text/plain".into(),
            data: "Explicit 中文".into(),
        },
        HostDragItem::File(HostFileData {
            name: "owned.bin".into(),
            media_type: "application/octet-stream".into(),
            last_modified: 123,
            bytes: vec![0, 255, 7],
        }),
    ];
    assert!(
        vm.dispatch_host_drag_event(family, &[key], &drop)
            .unwrap()
            .default_prevented
    );
    assert_eq!(run(&mut vm, "dropped && retained.getData('text') === '' && itemList.length === 0 && fileList.length === 0 && retainedFile.size === 3"), Value::Bool(true));
    run(&mut vm, "let bytes = ''; retainedFile.bytes().then(function(value) { bytes = Array.from(value).join(','); });");
    vm.run_promise_jobs().unwrap();
    assert_eq!(run(&mut vm, "bytes === '0,255,7'"), Value::Bool(true));
}

#[test]
fn get_as_string_queues_a_task_after_microtasks_and_keeps_the_requested_snapshot() {
    let (mut vm, _, _) = browser_vm();
    run(
        &mut vm,
        r#"
        let order = '', payload = '';
        const transfer = new DataTransfer();
        const item = transfer.items.add('Snapshot 中文', 'text/plain');
        item.getAsString(function(value) {
            order += 'task'; payload = value;
            Promise.resolve().then(function() { order += ':checkpoint'; });
        });
        transfer.items.clear();
        Promise.resolve().then(function() { order += 'microtask:'; });
    "#,
    );
    assert_eq!(
        run(&mut vm, "order === '' && payload === ''"),
        Value::Bool(true)
    );
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        run(&mut vm, "order === 'microtask:' && payload === ''"),
        Value::Bool(true)
    );
    vm.run_drag_tasks_bounded(1).unwrap();
    assert_eq!(
        run(
            &mut vm,
            "order === 'microtask:task:checkpoint' && payload === 'Snapshot 中文'"
        ),
        Value::Bool(true)
    );
}

#[test]
fn owner_drag_listener_removal_stop_and_exceptions_preserve_successor_events() {
    let (mut vm, family, child) = browser_vm();
    let parent = HostObjectKey::new(7, 2, 12);
    vm.install_host_object_factory("dragParent", 0, family, move |_: &[HostValue]| {
        Ok(Some(parent))
    })
    .unwrap();
    run(
        &mut vm,
        r#"
        const child = dragTarget(), parent = dragParent();
        let calls = '', retainedEvent;
        function removed(event) { calls += 'removed'; }
        child.addEventListener('dragover', function(event) {
            calls += 'child:'; retainedEvent = event;
            child.removeEventListener('dragover', removed);
            throw new Error('Page listener');
        });
        child.addEventListener('dragover', removed);
        child.addEventListener('dragover', function(event) { calls += 'stop'; event.stopImmediatePropagation(); event.preventDefault(); });
        child.addEventListener('dragover', function(event) { calls += 'extra'; });
        parent.addEventListener('dragover', function(event) { calls += 'parent'; });
        child.addEventListener('drop', function(event) { calls += ':drop'; event.preventDefault(); });
    "#,
    );
    let over = HostDragEventData::new(HostDragEventKind::DragOver, 1.0, 2.0);
    assert!(
        vm.dispatch_host_drag_event(family, &[child, parent], &over)
            .unwrap()
            .default_prevented
    );
    assert_eq!(run(&mut vm, "calls === 'child:stop' && retainedEvent.currentTarget === null && retainedEvent.defaultPrevented"), Value::Bool(true));
    let drop = HostDragEventData::new(HostDragEventKind::Drop, 1.0, 2.0);
    assert!(
        vm.dispatch_host_drag_event(family, &[child], &drop)
            .unwrap()
            .default_prevented
    );
    assert_eq!(
        run(&mut vm, "calls === 'child:stop:drop'"),
        Value::Bool(true)
    );
}

#[test]
fn owner_drag_dispatch_rejects_foreign_or_duplicate_ancestry_and_protected_payloads() {
    let (mut vm, family, key) = browser_vm();
    run(
        &mut vm,
        "let calls = 0; dragTarget().addEventListener('dragenter', function(event) { calls++; });",
    );
    let mut input = HostDragEventData::new(HostDragEventKind::DragEnter, 1.0, 2.0);
    input.items.push(HostDragItem::Text {
        format: "text/plain".into(),
        data: "Premature payload".into(),
    });
    assert!(vm.dispatch_host_drag_event(family, &[key], &input).is_err());
    input.items.clear();
    assert!(vm
        .dispatch_host_drag_event(family, &[key, key], &input)
        .is_err());
    assert!(vm
        .dispatch_host_drag_event(family, &[key, HostObjectKey::new(7, 3, 12)], &input)
        .is_err());
    let (_, foreign_family, _) = browser_vm();
    assert!(vm
        .dispatch_host_drag_event(foreign_family, &[key], &input)
        .is_err());
    assert_eq!(run(&mut vm, "calls === 0"), Value::Bool(true));
    vm.dispatch_host_drag_event(family, &[key], &input).unwrap();
    assert_eq!(run(&mut vm, "calls === 1"), Value::Bool(true));
}
