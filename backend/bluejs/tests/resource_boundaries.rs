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
        "Array.from(new Set([2, 1])).join(',') === '2,1'",
        "new Uint8Array([1, 2, 3]).slice(1).join(',') === '2,3'",
        "new ArrayBuffer(8).transfer(4).byteLength === 4",
        "new ArrayBuffer(8).transferToImmutable().byteLength === 8",
        "Temporal.PlainDate.from('2024-02-29').toString() === '2024-02-29'",
        "Temporal.PlainDateTime.from('2024-02-29T12:34').toString() === '2024-02-29T12:34:00'",
        "new Intl.ListFormat('en').format(['A', 'B']) === 'A and B'",
        "typeof new Intl.DurationFormat('en').format({ hours: 1 }) === 'string'",
        "(123.456).toFixed(2) === '123.46'",
        "JSON.stringify({ alpha: [1, 2] }) === '{\"alpha\":[1,2]}'",
        "(() => { let x = 3; return () => x + 1; })()() === 4",
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
