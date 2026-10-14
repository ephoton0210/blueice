// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public built-ins retain their normal result or report the configured heap
//! limit as allocations fail at different points in their execution.

use blueice_bluejs::{compile, parse, HeapError, RuntimeError, Value, Vm, VmConfig};

#[test]
fn builtins_across_subsystems_handle_heap_limits_at_allocation_boundaries() {
    let sources = [
        "Object.keys({ alpha: 1, beta: 2 }).join(',') === 'alpha,beta'",
        "new Map([[1, 'a'], [2, 'b']]).get(2) === 'b'",
        "new Map([[1, 2]]).entries().next().value.join(',') === '1,2'",
        "new Map([[1, 2]]).keys().next().value === 1",
        "new Map([[1, 2]]).values().next().value === 2",
        "(() => { let sum = 0; new Map([[1, 2]]).forEach(value => sum += value); return sum === 2; })()",
        "Array.from(new Set([2, 1])).join(',') === '2,1'",
        "new Set([1, 2]).entries().next().value.join(',') === '1,1'",
        "new Set([1, 2]).values().next().value === 1",
        "(() => { let sum = 0; new Set([1, 2]).forEach(value => sum += value); return sum === 3; })()",
        "new Uint8Array([1, 2, 3]).slice(1).join(',') === '2,3'",
        "new ArrayBuffer(8).transfer(4).byteLength === 4",
        "new ArrayBuffer(8).transferToImmutable().byteLength === 8",
        "new ArrayBuffer(8).transferToImmutable(12).byteLength === 12",
        "new ArrayBuffer(8).sliceToImmutable(2, 6).byteLength === 4",
        "new ArrayBuffer(8).sliceToImmutable().immutable === true",
        "new Uint8Array(new ArrayBuffer(8).transferToImmutable()).length === 8",
        "Temporal.PlainDate.from('2024-02-29').toString() === '2024-02-29'",
        "Temporal.PlainDateTime.from('2024-02-29T12:34').toString() === '2024-02-29T12:34:00'",
        "new Intl.ListFormat('en').format(['A', 'B']) === 'A and B'",
        "typeof new Intl.DurationFormat('en').format({ hours: 1 }) === 'string'",
        "new Intl.Collator('en').compare('a', 'b') < 0",
        "new Intl.Locale('en', { firstDayOfWeek: 2 }).firstDayOfWeek === 'tue'",
        "(123.456).toFixed(2) === '123.46'",
        "JSON.stringify({ alpha: [1, 2] }) === '{\"alpha\":[1,2]}'",
        "(() => { let x = 3; return () => x + 1; })()() === 4",
        "(function (a, b) { return a + b; }).bind(null, 1)(2) === 3",
        "(() => { function F() {} return new F() instanceof F; })()",
        "(() => { function F() {} return new F() instanceof F.bind(null); })()",
        "(() => { const f = new Proxy(function () {}, { get: Reflect.get }); return ({} instanceof f) === false; })()",
        "(function* () { yield 1; return 2; })().next().value === 1",
        "Promise.resolve(3).then(value => value + 1) instanceof Promise",
        "new Proxy({ x: 1 }, { get(target, key) { return target[key]; } }).x === 1",
        "new WeakMap([[{}, 1]]) instanceof WeakMap",
        "new Date(Date.UTC(2024, 1, 29)).toISOString() === '2024-02-29T00:00:00.000Z'",
        "new Intl.NumberFormat('en').format(1234) === '1,234'",
        "BigInt('123') + 1n === 124n",
        "Reflect.construct(Array, [1, 2]).join(',') === '1,2'",
        "Temporal.Duration.from({ hours: 25 }).round({ largestUnit: 'day' }).days === 1",
        "Temporal.Instant.from('2024-02-29T00:00Z').toString() === '2024-02-29T00:00:00Z'",
    ];

    for source in sources {
        let program = compile(&parse(source).unwrap()).unwrap();
        let mut probe = Vm::default();
        assert_eq!(
            probe.execute(&program).unwrap(),
            Value::Bool(true),
            "{source}"
        );
        let baseline = probe.heap().stats().managed_bytes;
        let mut completed = 0;
        let mut exhausted = 0;

        for limit in (baseline.saturating_sub(16_384)..=baseline + 16_384).step_by(64) {
            let mut config = VmConfig::default();
            config.heap.max_heap_bytes = limit;
            config.heap.major_threshold_bytes = config.heap.major_threshold_bytes.min(limit);
            let Ok(mut vm) = Vm::new(config) else {
                continue;
            };
            match vm.execute(&program) {
                Ok(Value::Bool(true)) => completed += 1,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => exhausted += 1,
                other => panic!("{source}, heap limit {limit}: {other:?}"),
            }
        }
        assert!(
            completed > 0 && exhausted > 0,
            "{source}, baseline {baseline}: {completed} completed, {exhausted} exhausted"
        );
    }
}

#[test]
fn foreign_array_producers_propagate_allocation_failures_after_realm_setup() {
    let setup = compile(&parse("globalThis.other = $262.createRealm().global").unwrap()).unwrap();
    for source in [
        "other.Array.prototype.toReversed.call([1,2,3]).join(',') === '3,2,1'",
        "other.Array.prototype.toSorted.call([3,1,2]).join(',') === '1,2,3'",
        "other.Array.prototype.toSpliced.call([1,2,3],1,1,4).join(',') === '1,4,3'",
        "other.Array.prototype.with.call([1,2,3],1,4).join(',') === '1,4,3'",
        "other.Array.prototype.map.call([1,2,3], x => x + 1).join(',') === '2,3,4'",
        "other.Iterator.prototype.toArray.call([1,2,3].values()).join(',') === '1,2,3'",
    ] {
        let program = compile(&parse(source).unwrap()).unwrap();
        let mut probe = Vm::default();
        probe.install_test262_harness().unwrap();
        probe.execute(&setup).unwrap();
        let setup_bytes = probe.heap().stats().managed_bytes;
        assert_eq!(
            probe.execute(&program).unwrap(),
            Value::Bool(true),
            "{source}"
        );
        let completed_bytes = probe.heap().stats().managed_bytes;
        let mut completed = 0;
        let mut exhausted = 0;

        for limit in (setup_bytes.saturating_sub(4_096)..=completed_bytes + 32_768).step_by(64) {
            let mut config = VmConfig::default();
            config.heap.max_heap_bytes = limit;
            config.heap.major_threshold_bytes = config.heap.major_threshold_bytes.min(limit);
            let Ok(mut vm) = Vm::new(config) else {
                continue;
            };
            match vm
                .install_test262_harness()
                .and_then(|()| vm.execute(&setup).map(|_| ()))
            {
                Ok(()) => {}
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => continue,
                other => panic!("{source}, realm setup limit {limit}: {other:?}"),
            }
            match vm.execute(&program) {
                Ok(Value::Bool(true)) => completed += 1,
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => exhausted += 1,
                other => panic!("{source}, method limit {limit}: {other:?}"),
            }
        }
        assert!(
            completed > 0 && exhausted > 0,
            "{source}: setup {setup_bytes}, completed {completed_bytes}, {completed} completed, {exhausted} exhausted"
        );
    }
}
