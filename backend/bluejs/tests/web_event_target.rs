// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Behavioral reference: https://dom.spec.whatwg.org/#interface-eventtarget
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

#[test]
fn installing_file_reader_preserves_each_browser_constructor_under_collection() {
    let mut vm = web_vm();
    for (name, source) in [
        ("EventTarget", "new EventTarget() instanceof EventTarget"),
        ("Event", "new Event('x') instanceof Event"),
        ("ProgressEvent", "new ProgressEvent('x') instanceof Event"),
        ("FileReader", "new FileReader() instanceof EventTarget"),
    ] {
        let program = compile(&parse(source).unwrap()).unwrap();
        assert_eq!(vm.execute_script(&program), Ok(Value::Bool(true)), "{name}");
    }
}

#[test]
fn event_target_brands_and_dispatch_keep_event_identity_and_cancellation() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        const target = new EventTarget();
        const event = new Event('custom', {cancelable:true});
        let called = false, rejected = 0;
        target.addEventListener('custom', function(received) {
            called = received === event && this === target && received.target === target
                && received.currentTarget === target && received.eventPhase === Event.AT_TARGET
                && received.composedPath()[0] === target && !received.isTrusted;
            received.preventDefault();
        });
        for (const action of [
            () => EventTarget(),
            () => EventTarget.prototype.addEventListener.call({}, 'custom', () => {}),
            () => target.dispatchEvent({type:'custom'})
        ]) { try { action(); } catch(error) { rejected++; } }
        !target.dispatchEvent(event) && called && rejected === 3 && event.defaultPrevented
            && target.dispatchEvent(new Event(''))
            && event.currentTarget === null && event.eventPhase === 0 && event.target === target
            && event.composedPath().length === 0
            && Object.prototype.toString.call(target) === '[object EventTarget]'
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn listener_capture_identity_once_removal_and_reentrant_dispatch_use_live_snapshots() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        const target = new EventTarget(), calls = [];
        function capture() { calls.push('capture'); }
        function removed() { calls.push('removed'); }
        target.addEventListener('x', capture, true);
        target.addEventListener('x', capture, {capture:true,once:true});
        target.addEventListener('x', function once() {
            calls.push('once');
            target.removeEventListener('x', removed);
            target.addEventListener('x', () => calls.push('late'));
            target.dispatchEvent(new Event('x'));
        }, {once:true});
        target.addEventListener('x', removed);
        target.dispatchEvent(new Event('x'));
        target.removeEventListener('x', capture, true);
        calls.join(',') === 'capture,once,capture,late'
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn object_listeners_lookup_handle_event_at_dispatch_and_survive_gc() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        const target = new EventTarget();
        let lookup = 0, called = 0;
        const callback = {get handleEvent() {
            lookup++;
            return function(event) { if (this === callback && event.type === 'x') called++; };
        }};
        target.addEventListener('x', callback);
        for (let i=0;i<100;i++) new Blob(['Garbage ' + i]);
        target.dispatchEvent(new Event('x'));
        target.dispatchEvent(new Event('x'));
        target.removeEventListener('x', callback);
        target.dispatchEvent(new Event('x'));
        lookup === 2 && called === 2
    "#
        ),
        Value::Bool(true)
    );
}

#[test]
fn passive_exceptions_immediate_stop_and_redispatch_keep_dispatch_state_consistent() {
    assert_eq!(
        run(
            &mut web_vm(),
            r#"
        const target = new EventTarget(), calls = [];
        const event = new Event('x', {cancelable:true});
        target.addEventListener('x', e => { e.preventDefault(); calls.push(e.defaultPrevented); }, {passive:true});
        target.addEventListener('x', e => {
            try { target.dispatchEvent(e); } catch(error) { calls.push(error.name); }
            throw new Error('Page listener failure');
        });
        target.addEventListener('x', e => { calls.push('stop'); e.stopImmediatePropagation(); });
        target.addEventListener('x', e => calls.push('unreachable'));
        const first = target.dispatchEvent(event), second = target.dispatchEvent(event);
        first && second && event.currentTarget === null && !event.cancelBubble
            && calls.join(',') === 'false,InvalidStateError,stop,false,InvalidStateError,stop'
    "#
        ),
        Value::Bool(true)
    );
}
