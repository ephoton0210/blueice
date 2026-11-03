// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Allocation sites that gather the VM's roots only when the heap has to collect (cells for
//! captured variables, closures, object and array literals, `for-in` bookkeeping, legacy
//! function properties) must produce the same answers when a collection happens at every one
//! of them. A one-object nursery and a tiny major threshold make that so.

use blueice_bluejs::{compile, parse, HeapConfig, Value, Vm, VmConfig};

fn run(source: &str, nursery: usize) -> Value {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: nursery,
            major_threshold_bytes: 2 * 1024,
            ..HeapConfig::default()
        },
        instruction_budget: 200_000_000,
        ..VmConfig::default()
    })
    .unwrap();
    vm.execute(&compile(&parse(source).expect(source)).expect(source))
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

fn text(value: Value) -> String {
    match value {
        Value::String(text) => text.to_utf8().unwrap(),
        other => panic!("{other:?}"),
    }
}

/// The same program under a generous heap and under constant collection must agree.
fn agrees(source: &str) -> String {
    let relaxed = text(run(source, 100_000));
    for nursery in [1usize, 2, 3, 7] {
        assert_eq!(text(run(source, nursery)), relaxed, "nursery {nursery}: {source}");
    }
    relaxed
}

#[test]
fn captured_variables_and_closures_survive_collection_at_every_allocation() {
    let result = agrees(
        "(function () {
            let out = [];
            for (let i = 0; i < 40; i++) {
                let captured = { n: i, tag: 'v' + i };
                const read = () => captured.n + captured.tag.length;
                const bump = function () { captured.n += 1; return captured.n; };
                bump();
                out.push(read());
            }
            return JSON.stringify(out);
        })()",
    );
    assert!(result.starts_with("[3,4,5,"), "{result}");
}

#[test]
fn object_and_array_literals_and_for_in_survive_collection() {
    let result = agrees(
        "(function () {
            const src = { a: 1, b: { c: [1, 2, { d: 4 }] }, e: 'x' };
            let keys = [];
            for (const k in src) { keys.push(k + ':' + JSON.stringify({ k, v: [src[k]] })); }
            let nested = [];
            for (let i = 0; i < 30; i++) { nested.push({ i, arr: [i, [i * 2]], o: { p: { q: i } } }); }
            return keys.join('|') + '#' + JSON.stringify(nested[29]);
        })()",
    );
    assert!(result.contains("a:{\"k\":\"a\",\"v\":[1]}"), "{result}");
    assert!(result.ends_with("\"q\":29}}}"), "{result}");
}

#[test]
fn legacy_function_properties_and_arguments_survive_collection() {
    let result = agrees(
        "(function () {
            function f(a, b) { return arguments.length + ':' + (typeof f.caller) + ':' + (typeof f.arguments); }
            let out = [];
            for (let i = 0; i < 20; i++) { out.push(f(i, { x: i })); }
            function g() { return Array.prototype.slice.call(arguments).map((v) => typeof v).join(','); }
            return out[0] + '|' + out[19] + '|' + g(1, 'a', {}, [], null, undefined);
        })()",
    );
    assert!(result.contains("2:"), "{result}");
    assert!(result.ends_with("number,string,object,object,object,undefined"), "{result}");
}

#[test]
fn iterator_records_survive_collection_for_of_spread_and_destructuring() {
    let result = agrees(
        "(function () {
            const list = Array.from({ length: 25 }, (_, i) => ({ i, s: 'item' + i }));
            let a = 0;
            for (const { i, s } of list) a += i + s.length;
            const [first, , third, ...rest] = list;
            const spread = [...list, ...'xyz', ...new Set([1, 2, 2, 3])];
            const m = new Map(list.map((o) => [o.s, o]));
            let viaMap = 0;
            for (const [k, v] of m) viaMap += k.length + v.i;
            function* gen() { for (const o of list) yield o.i; }
            return [a, first.s, third.s, rest.length, spread.length, viaMap, [...gen()].length].join(',');
        })()",
    );
    assert_eq!(result, "440,item0,item2,22,31,440,25");
}

#[test]
fn regexp_results_and_string_splits_survive_collection() {
    let result = agrees(
        "(function () {
            const re = /(?<year>\\d{4})-(?<month>\\d{2})/dg;
            const text = '2020-01 and 2021-02 and 2022-03';
            const found = [];
            let m;
            while ((m = re.exec(text))) found.push(m.groups.year + '/' + m.groups.month + '@' + m.indices.groups.year[0] + '#' + m.indices.length);
            const parts = 'a,b;c d,e;f g'.split(/[,; ]/);
            const all = [...text.matchAll(/\\d+/g)].map((x) => x[0]);
            return found.join('|') + ':' + parts.join('') + ':' + all.length + ':' + text.replace(/(\\d{4})/g, '<$1>');
        })()",
    );
    assert_eq!(
        result,
        "2020/01@0#3|2021/02@12#3|2022/03@24#3:abcdefg:6:<2020>-01 and <2021>-02 and <2022>-03"
    );
}
