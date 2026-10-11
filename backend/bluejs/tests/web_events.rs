// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Behavioral references: DOM, UI Events, Pointer Events and CSSOM View.
use blueice_bluejs::{compile, parse, HostObjectKey, HostValue, Value, Vm, VmConfig};

fn browser_vm() -> Vm {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    let mut vm = Vm::new(config).unwrap();
    vm.install_web_file_api().unwrap();
    let family = vm.create_host_object_family().unwrap();
    vm.install_host_file_selection_event_methods(family)
        .unwrap();
    vm.install_host_object_factory("eventTarget", 0, family, |_: &[HostValue]| {
        Ok(Some(HostObjectKey::new(9, 2, 5)))
    })
    .unwrap();
    let opaque_family = vm.create_host_object_family().unwrap();
    vm.install_host_object_factory("opaqueHost", 0, opaque_family, |_: &[HostValue]| {
        Ok(Some(HostObjectKey::new(9, 2, 6)))
    })
    .unwrap();
    vm
}

fn check(source: &str) {
    let code = compile(&parse(source).unwrap()).unwrap();
    assert_eq!(
        browser_vm().execute_script(&code).unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn event_interfaces_preserve_constructor_and_prototype_inheritance_and_brands() {
    check(
        r#"
        const drag = new DragEvent('drop');
        const mouse = new MouseEvent('click');
        const ui = new UIEvent('focus');
        const event = new Event('ready');
        let forged = 0;
        for (const [prototype, property, receiver] of [
            [Event.prototype, 'type', {}],
            [UIEvent.prototype, 'detail', event],
            [MouseEvent.prototype, 'clientX', ui],
            [DragEvent.prototype, 'dataTransfer', mouse],
            [MouseEvent.prototype, 'clientX', Object.create(MouseEvent.prototype)]
        ]) {
            try { Object.getOwnPropertyDescriptor(prototype, property).get.call(receiver); }
            catch (error) { if (error instanceof TypeError) forged++; }
        }
        drag instanceof DragEvent && drag instanceof MouseEvent && drag instanceof UIEvent
            && drag instanceof Event && mouse instanceof UIEvent && ui instanceof Event
            && !(event instanceof UIEvent) && !(mouse instanceof DragEvent) && forged === 5
            && Object.getPrototypeOf(DragEvent) === MouseEvent
            && Object.getPrototypeOf(MouseEvent) === UIEvent
            && Object.getPrototypeOf(UIEvent) === Event
            && Object.getPrototypeOf(DragEvent.prototype) === MouseEvent.prototype
            && Object.prototype.toString.call(drag) === '[object DragEvent]'
            && Event.CAPTURING_PHASE === 1 && drag.NONE === 0
    "#,
    );
}

#[test]
fn event_readonly_attributes_use_branded_getters_and_trust_is_unforgeable() {
    check(
        r#"
        const event = new DragEvent('drop', {bubbles: true, cancelable: true, clientX: 3.5});
        const trust = Object.getOwnPropertyDescriptor(event, 'isTrusted');
        const x = Object.getOwnPropertyDescriptor(MouseEvent.prototype, 'clientX');
        let forged = false;
        try { trust.get.call({}); } catch (error) { forged = error instanceof TypeError; }
        event.clientX = 99; event.isTrusted = true; event.type = 'changed';
        !Object.hasOwn(event, 'clientX') && !Object.hasOwn(event, 'type')
            && typeof x.get === 'function' && x.set === undefined && x.enumerable && x.configurable
            && typeof trust.get === 'function' && trust.set === undefined && !trust.configurable
            && trust.enumerable && forged && event.clientX === 3.5 && event.type === 'drop'
            && event.isTrusted === false && event.target === null && event.currentTarget === null
            && event.eventPhase === Event.NONE && event.timeStamp >= 0
            && event.composedPath().length === 0 && event.dataTransfer === null
    "#,
    );
}

#[test]
fn mouse_event_coordinates_keep_fractions_and_idl_integers_wrap() {
    check(
        r#"
        const event = new DragEvent('drag', {clientX: 1.75, clientY: -2.25, screenX: 9.5,
            screenY: -4.5, button: 65535, buttons: -1, detail: 4294967295,
            altKey: 1, modifierAltGraph: true, modifierCapsLock: true});
        let invalid = 0;
        for (const field of ['clientX', 'clientY', 'screenX', 'screenY']) {
            for (const value of [NaN, Infinity, -Infinity]) {
                try { new MouseEvent('move', {[field]: value}); }
                catch (error) { if (error instanceof TypeError) invalid++; }
            }
        }
        event.clientX === 1.75 && event.clientY === -2.25 && event.x === 1.75 && event.y === -2.25
            && event.screenX === 9.5 && event.screenY === -4.5 && event.pageX === 1.75
            && event.pageY === -2.25 && event.offsetX === 1.75 && event.offsetY === -2.25
            && event.button === -1 && event.buttons === 65535 && event.detail === -1
            && event.altKey && event.getModifierState('Alt') && event.getModifierState('AltGraph')
            && event.getModifierState('CapsLock') && !event.getModifierState('Unknown')
            && invalid === 12 && new MouseEvent('x', {button: Infinity}).button === 0
    "#,
    );
}

#[test]
fn event_dictionary_conversion_observes_inheritance_order_and_rejects_forged_targets() {
    check(
        r#"
        const reads = [];
        const options = new Proxy({}, {get(target, key) { reads.push(key); return undefined; }});
        new DragEvent('drop', options);
        const expected = ['bubbles','cancelable','composed','detail','view',
            'altKey','ctrlKey','metaKey','modifierAltGraph','modifierCapsLock','modifierFn',
            'modifierFnLock','modifierHyper','modifierNumLock','modifierScrollLock','modifierSuper',
            'modifierSymbol','modifierSymbolLock','shiftKey','button','buttons','clientX','clientY',
            'relatedTarget','screenX','screenY','dataTransfer'];
        let rejected = 0;
        for (const options of [{relatedTarget: {}}, {relatedTarget: opaqueHost()}, {view: {}}, {dataTransfer: {}}, 1]) {
            try { new DragEvent('drop', options); }
            catch (error) { if (error instanceof TypeError) rejected++; }
        }
        reads.join(',') === expected.join(',') && rejected === 5
            && new UIEvent('focus', null).view === null
            && new Event(undefined).type === 'undefined'
    "#,
    );
}

#[test]
fn event_cancellation_aliases_and_reinitialization_keep_subclass_and_gc_identity() {
    check(
        r#"
        class CustomDrag extends DragEvent {}
        const target = eventTarget();
        const transfer = new DataTransfer(); transfer.setData('text/plain', 'owned 中文');
        const event = new CustomDrag('drop', {dataTransfer: transfer, relatedTarget: target,
            bubbles: true, cancelable: true, ctrlKey: true});
        event.cancelBubble = true; event.cancelBubble = false; event.returnValue = false;
        const canceled = event.cancelBubble && !event.returnValue && event.defaultPrevented;
        event.initEvent('newtype', false, false);
        event.preventDefault(); event.returnValue = false;
        for (let i = 0; i < 40; i++) { new DragEvent('gc', {dataTransfer: new DataTransfer()}); }
        canceled && event instanceof CustomDrag && event instanceof MouseEvent
            && event.type === 'newtype' && !event.bubbles && !event.cancelable
            && !event.cancelBubble && event.returnValue && !event.defaultPrevented && !event.isTrusted
            && event.relatedTarget === target && event.dataTransfer === transfer
            && event.dataTransfer.getData('text') === 'owned 中文' && event.getModifierState('Control')
            && Object.getOwnPropertyDescriptor(MouseEvent.prototype, 'relatedTarget').get.call(event) === target
    "#,
    );
}

#[test]
fn legacy_ui_and_mouse_initializers_preserve_subclasses_and_convert_idl_arguments() {
    check(
        r#"
        const transfer = new DataTransfer(); transfer.setData('text/plain', 'retained');
        const related = eventTarget();
        class Child extends DragEvent {}
        const event = new Child('drop', {dataTransfer: transfer, buttons: 5, composed: true,
            modifierCapsLock: true, cancelable: true});
        event.preventDefault(); event.stopImmediatePropagation();
        event.initMouseEvent('legacy', 1, true, null, 4294967295, 1.75, -2.75,
            4294967297, Infinity, true, false, true, false, 65535, related);
        const first = event.type === 'legacy' && event.bubbles && event.cancelable
            && !event.defaultPrevented && !event.cancelBubble && !event.isTrusted
            && event.screenX === 1 && event.screenY === -2 && event.clientX === 1
            && event.clientY === 0 && event.detail === -1 && event.button === -1
            && event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey
            && event.relatedTarget === related && event.buttons === 5 && event.composed
            && event.getModifierState('CapsLock') && event.dataTransfer === transfer;
        event.initUIEvent('ui');
        const second = event.type === 'ui' && event.detail === 0 && event.view === null
            && !event.bubbles && !event.cancelable && event.relatedTarget === related;
        event.initMouseEvent('defaults');
        for (let i = 0; i < 40; i++) { new DragEvent('gc', {dataTransfer: new DataTransfer()}); }
        first && second && event instanceof Child && event.dataTransfer === transfer
            && event.dataTransfer.getData('text') === 'retained'
            && event.clientX === 0 && event.clientY === 0 && event.screenX === 0
            && event.button === 0 && event.relatedTarget === null && !event.ctrlKey
            && UIEvent.prototype.initUIEvent.length === 1
            && MouseEvent.prototype.initMouseEvent.length === 1
    "#,
    );
}

#[test]
fn legacy_initializers_validate_brands_and_convert_before_mutating_event_state() {
    check(
        r#"
        const event = new DragEvent('original', {clientX: 9.5, cancelable: true});
        const reads = [];
        const number = name => ({valueOf() { reads.push(name); return 1; }});
        event.initMouseEvent({toString() { reads.push('type'); return 'converted'; }},
            false, false, null, number('detail'), number('screenX'), number('screenY'),
            number('clientX'), number('clientY'), false, false, false, false, number('button'));
        let rejected = 0;
        for (const action of [
            () => UIEvent.prototype.initUIEvent.call(new Event('x'), 'bad'),
            () => MouseEvent.prototype.initMouseEvent.call(new UIEvent('x'), 'bad'),
            () => event.initUIEvent(),
            () => event.initMouseEvent(),
            () => event.initUIEvent('bad', false, false, {}),
            () => event.initMouseEvent('bad', false, false, null, 0, 0, 0, 0, 0,
                false, false, false, false, 0, opaqueHost())
        ]) {
            try { action(); } catch (error) { if (error instanceof TypeError) rejected++; }
        }
        let thrown = false;
        try { event.initUIEvent('bad', false, false, null,
            {valueOf() { event.preventDefault(); throw new Error('conversion'); }}); }
        catch (error) { thrown = error.message === 'conversion'; }
        reads.join(',') === 'type,detail,screenX,screenY,clientX,clientY,button'
            && rejected === 6 && thrown && event.type === 'converted' && event.clientX === 1
    "#,
    );
}
