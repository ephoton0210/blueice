// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn harness_assertions_fail_closed() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "assert(true)",
        "assert.sameValue(print('diagnostic'),undefined)",
        "assert.sameValue(NaN,NaN)",
        "assert.notSameValue(0,-0)",
        "assert.throws(TypeError,()=>''.repeat.call(null))",
        "assert.throws(RangeError,()=>''.repeat(-1))",
        "assert.throws(Error,()=>{throw new Error('x');})",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .unwrap(),
            Value::Undefined,
            "{source}"
        );
    }
    for source in [
        "assert(false)",
        "assert(1)",
        "assert.sameValue(0,-0)",
        "assert.notSameValue(NaN,NaN)",
        "assert.throws(TypeError,()=>1)",
        "assert.throws(TypeError,()=>{throw 1;})",
        "assert.throws(TypeError,()=>{throw new Error();})",
        "$DONOTEVALUATE()",
    ] {
        assert!(
            matches!(
                vm.execute(&compile(&parse(source).unwrap()).unwrap()),
                Err(RuntimeError::Test262(_))
            ),
            "{source}"
        );
    }
    assert!(matches!(
        Vm::default().execute(&compile(&parse("assert(true)").unwrap()).unwrap()),
        Err(RuntimeError::ReferenceError(_))
    ));
}

#[test]
fn object_entries_roots_intermediate_temporal_range_arguments() {
    let template = r#"
        const us = new Intl.DateTimeFormat('en-US');
        const instances = {
          date: new Date(1580527800000),
          instant: new Temporal.Instant(0n),
          plaindate: new Temporal.PlainDate(2000, 5, 2),
          plaindatetime: new Temporal.PlainDateTime(2000, 5, 2, 12, 34, 56, 987, 654, 321),
          plainmonthday: new Temporal.PlainMonthDay(5, 2),
          plaintime: new Temporal.PlainTime(13, 37),
          plainyearmonth: new Temporal.PlainYearMonth(2019, 6),
          zoneddatetime: new Temporal.ZonedDateTime(0n, 'America/Kentucky/Louisville')
        };

        Object.entries(instances).forEach(([typeName, instance]) => {
          Object.entries(instances).forEach(([anotherTypeName, anotherInstance]) => {
            if (typeName !== anotherTypeName) {
              assert.throws(
                TypeError,
                () => { us.METHOD(instance, anotherInstance); },
                'bad arguments (' + typeName + ' and ' + anotherTypeName + ')'
              );
            }
          });
        });
    "#;

    for method in ["formatRange", "formatRangeToParts"] {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        let source = template.replace("METHOD", method);
        assert_eq!(
            vm.execute(&compile(&parse(&source).unwrap()).unwrap()),
            Ok(Value::Undefined),
            "{method}"
        );
    }
}

#[test]
fn property_helper_observes_writes_and_receiver_setters() {
    let mut vm = super::harness::property_helper_vm();
    let source = r#"
        let sealed = Object.preventExtensions({});
        let accessor = {};
        Object.defineProperty(accessor, "value", {
            get() { return 0; },
            set(value) { accessor.received = value; },
            configurable: true,
        });
        let existing = { value: "unlikelyValue" };
        let created = {};
        let locked = {};
        Object.defineProperty(locked, "value", { value: 1 });
        let throwing = {};
        Object.defineProperty(throwing, "value", {
            set(value) { throw new Error("setter error"); },
            configurable: true,
        });
        verifyNotWritable(sealed, "newProperty", "noWrite");
        verifyWritable(accessor, "value", "received", "written");
        verifyWritable(existing, "value");
        verifyWritable(created, "created", "created");
        verifyWritable([], "length");
        verifyNotWritable(locked, "value");
        // No own descriptor to read `writable` from: upstream dereferences it.
        assert.throws(TypeError, () => verifyWritable({}, "missing"));
        assert.throws(Test262Error, () => verifyWritable(locked, "value"));
        assert.throws(Test262Error, () => verifyNotWritable(existing, "value", "value"));
        assert.throws(Test262Error, () => verifyWritable(throwing, "value", "value"));
        sealed.newProperty === undefined && accessor.received === 0 && created.created === undefined
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );

    let strict = r#"
        "use strict";
        verifyNotWritable(Object.preventExtensions({}), "newProperty", "noWrite");
        true
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(strict).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
}

#[test]
fn date_constructor_uses_its_new_target_prototype() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let prototype = {};
        function NewTarget() {}
        NewTarget.prototype = prototype;
        let direct = new Date(0);
        let reflected = Reflect.construct(Date, [], NewTarget);
        Object.getPrototypeOf(direct) === Date.prototype &&
          Object.getPrototypeOf(reflected) === prototype &&
          direct instanceof Date && reflected instanceof NewTarget
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
}

#[test]
fn weak_collection_constructors_use_a_foreign_new_target_realm_prototype() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        var other = $262.createRealm().global;
        var C = new other.Function();
        C.prototype = null;
        var map = Reflect.construct(WeakMap, [], C);
        var set = Reflect.construct(WeakSet, [], C);
        Object.getPrototypeOf(map) === other.WeakMap.prototype &&
        Object.getPrototypeOf(set) === other.WeakSet.prototype;
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
}

#[test]
fn test262_agents_share_bytes_wait_and_report_in_notify_order() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(12);
        let ints = new Int32Array(buffer);
        $262.agent.start(`
            $262.agent.receiveBroadcast(function (shared) {
                let view = new Int32Array(shared);
                Atomics.add(view, 0, 1);
                $262.agent.report(Atomics.wait(view, 1, 0, 10000));
                $262.agent.leaving();
            });
        `);
        $262.agent.broadcast(buffer);
        while (Atomics.load(ints, 0) !== 1) {}
        let woken = 0;
        while (woken === 0) {
            woken = Atomics.notify(ints, 1, 1);
            if (woken === 0) $262.agent.sleep(1);
        }
        woken === 1
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
    let report = compile(
        &parse(
            r#"
                let report = null;
                while (report === null) {
                    report = $262.agent.getReport();
                    if (report === null) $262.agent.sleep(1);
                }
                report
            "#,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(vm.execute(&report), Ok(Value::String("ok".into())));
    assert_eq!(vm.shutdown_test262_agents(), Ok(()));
}

#[test]
fn test262_agents_broadcast_before_wait_preserves_shared_spin_protocol() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(12);
        let ints = new Int32Array(buffer);
        $262.agent.start(`
            $262.agent.receiveBroadcast(function (shared) {
                let view = new Int32Array(shared);
                Atomics.add(view, 2, 1);
                while (Atomics.load(view, 1) === 0) {}
                $262.agent.report(7);
                Atomics.wait(view, 0, 0);
                $262.agent.report(8);
                $262.agent.leaving();
            });
        `);
        $262.agent.broadcast(buffer);
        while (Atomics.load(ints, 2) !== 1) {}
        Atomics.store(ints, 1, 1);
        let woken = 0;
        while (woken === 0) {
            woken = Atomics.notify(ints, 0, 1);
            if (woken === 0) $262.agent.sleep(1);
        }
        woken === 1
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
    let report = compile(
        &parse(
            r#"
                let reports = [];
                while (reports.length !== 2) {
                    let report = $262.agent.getReport();
                    if (report === null) $262.agent.sleep(1);
                    else reports.push(report);
                }
                reports.join('')
            "#,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(vm.execute(&report), Ok(Value::String("78".into())));
    assert_eq!(vm.shutdown_test262_agents(), Ok(()));
}

#[test]
fn test262_agents_receive_each_broadcast_in_host_order() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let first = new SharedArrayBuffer(4);
        let second = new SharedArrayBuffer(4);
        Atomics.store(new Int32Array(first), 0, 1);
        Atomics.store(new Int32Array(second), 0, 2);
        $262.agent.start(`
            let values = [];
            $262.agent.receiveBroadcast(shared => {
                values.push(Atomics.load(new Int32Array(shared), 0));
            });
            $262.agent.receiveBroadcast(shared => {
                values.push(Atomics.load(new Int32Array(shared), 0));
            });
            $262.agent.report(values.join(','));
            $262.agent.leaving();
        `);
        $262.agent.broadcast(first);
        $262.agent.broadcast(second);
        let report = null;
        while ((report = $262.agent.getReport()) === null) {
            $262.agent.sleep(1);
        }
        report
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::String("1,2".into()))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn test262_agents_notify_wakes_fifo_waiters() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(20);
        let ints = new Int32Array(buffer);
        for (let i = 0; i < 3; i++) {
            $262.agent.start(`
                $262.agent.receiveBroadcast(function (shared) {
                    let view = new Int32Array(shared);
                    Atomics.add(view, 4, 1);
                    while (Atomics.load(view, 1 + ${i}) === 0) {}
                    $262.agent.report(${i});
                    Atomics.wait(view, 0, 0);
                    $262.agent.report(${i});
                    $262.agent.leaving();
                });
            `);
        }
        $262.agent.broadcast(buffer);
        while (Atomics.load(ints, 4) !== 3) {}
        for (let i = 0; i < 3; i++) {
            Atomics.store(ints, 1 + i, 1);
            let report = null;
            while (report === null) {
                report = $262.agent.getReport();
                if (report === null) $262.agent.sleep(1);
            }
        }
        let woken = 0;
        while (woken !== 3) {
            woken += Atomics.notify(ints, 0, 3 - woken);
            if (woken !== 3) $262.agent.sleep(1);
        }
        woken
    "#;
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Number(3.0))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn test262_agents_atomic_read_modify_write_does_not_lose_updates() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        const workers = 8;
        const rounds = 200;
        let buffer = new SharedArrayBuffer(8);
        let view = new Int32Array(buffer);
        for (let worker = 0; worker < workers; worker++) {
            $262.agent.start(`
                $262.agent.receiveBroadcast(shared => {
                    let agent_view = new Int32Array(shared);
                    for (let round = 0; round < ${rounds}; round++) {
                        Atomics.add(agent_view, 0, 1);
                    }
                    Atomics.add(agent_view, 1, 1);
                    $262.agent.leaving();
                });
            `);
        }
        $262.agent.broadcast(buffer);
        while (Atomics.load(view, 1) !== workers) {}
        Atomics.load(view, 0)
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Number(1_600.0))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn atomics_wait_async_returns_a_promise_and_settles_on_the_vm_thread() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    let source = r#"
        assert.sameValue(typeof Atomics.waitAsync, 'function');
        let view = new Int32Array(new SharedArrayBuffer(4));
        let { async, value } = Atomics.waitAsync(view, 0, 0, 1_000);
        assert.sameValue(async, true);
        assert(value instanceof Promise);
        assert.sameValue(Object.getPrototypeOf(value), Promise.prototype);
        value.then(status => {
            assert.sameValue(status, 'ok');
        }).then(() => $DONE(), $DONE);
        Atomics.add(view, 0, 1);
        Atomics.notify(view, 0, 1);
    "#;
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    assert_eq!(vm.run_test262_async_until_done(), Ok(Some(Ok(()))));
}

#[test]
fn async_completion_returns_timeout_after_the_event_loop_becomes_quiescent() {
    let mut vm = Vm::default();
    vm.install_test262_done().unwrap();
    vm.execute_script(&compile(&parse("1").unwrap()).unwrap())
        .unwrap();

    assert_eq!(vm.run_test262_async_until_done(), Ok(None));
}

#[test]
fn async_completion_waits_for_a_registered_host_timer() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_done().unwrap();
    vm.execute_script(&compile(&parse("setTimeout($DONE, 1)").unwrap()).unwrap())
        .unwrap();

    assert_eq!(vm.run_test262_async_until_done(), Ok(Some(Ok(()))));
}

#[test]
fn test262_agents_wait_async_registers_two_waiters_before_notify() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(12);
        let view = new Int32Array(buffer);
        for (let label of ['A', 'B']) {
            $262.agent.start(`
                $262.agent.receiveBroadcast(async function (shared) {
                    let agent_view = new Int32Array(shared);
                    Atomics.add(agent_view, 1, 1);
                    let wait = Atomics.waitAsync(agent_view, undefined, 0);
                    Atomics.add(agent_view, 2, 1);
                    $262.agent.report("${label} " + await wait.value);
                    $262.agent.leaving();
                });
            `);
        }
        $262.agent.broadcast(buffer);
        while (Atomics.load(view, 2) !== 2) {}
        Atomics.notify(view, 0, 2)
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Number(2.0))
    );
    let reports = compile(
        &parse(
            r#"
                let reports = [];
                while (reports.length !== 2) {
                    let report = $262.agent.getReport();
                    if (report === null) {
                        $262.agent.sleep(1);
                    } else {
                        reports.push(report);
                    }
                }
                reports.sort().join(',')
            "#,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm.execute_script(&reports),
        Ok(Value::String("A ok,B ok".into()))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn test262_agent_reports_immediate_bigint_wait_async_result() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = r#"
        let buffer = new SharedArrayBuffer(32);
        let view = new BigInt64Array(buffer);
        $262.agent.start(`
            $262.agent.receiveBroadcast(function (shared) {
                let agent_view = new BigInt64Array(shared);
                Atomics.add(agent_view, 1, 1n);
                $262.agent.report(Atomics.store(agent_view, 0, 42n));
                $262.agent.report(Atomics.waitAsync(agent_view, 0, 0n).value);
                $262.agent.leaving();
            });
        `);
        $262.agent.broadcast(buffer);
        while (Atomics.load(view, 1) !== 1n) {}
        let reports = [];
        while (reports.length !== 2) {
            let report = $262.agent.getReport();
            if (report === null) $262.agent.sleep(1);
            else reports.push(report);
        }
        reports.join(',')
    "#;
    assert_eq!(
        vm.execute_script(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::String("42,not-equal".into()))
    );
    vm.shutdown_test262_agents().unwrap();
}

#[test]
fn generated_regexp_class_escape_helper_preserves_regexp_verdicts() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    for source in [
        "__bluejsTest262RegExpClassEscape([/^a+$/, /^a+$/], 'aaa', true)",
        "__bluejsTest262RegExpClassEscape([/b/, /c/], 'aaa', false)",
        "__bluejsTest262RegExpClassEscape([/^\\D+$/, /^\\D+$/u, /^\\D+$/v], String.fromCodePoint(0x10000, 0x10001), true)",
    ] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap()),
            Ok(Value::Bool(true)),
            "{source}"
        );
    }
    assert!(matches!(
        vm.execute(
            &compile(&parse("__bluejsTest262RegExpClassEscape([/a/], 'aaa', false)").unwrap())
                .unwrap()
        ),
        Err(RuntimeError::Test262(_))
    ));
}

#[test]
fn typed_array_overlap_helper_uses_the_real_set_operation() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let bytes=new Uint8Array(32);let doubles=new Float64Array(bytes.buffer,0,4);__bluejsTest262TypedArrayOverlappingSet(bytes,doubles)";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Bool(true))
    );
}
