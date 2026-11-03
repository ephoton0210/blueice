// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `JSON.parse` builds one object, array and property per node of the text. Each of those
//! must cost what it allocates, not what the whole VM holds: gathering the VM's roots at every
//! node made parsing a megabyte of JSON take half a minute.

use blueice_bluejs::{compile, parse, HeapConfig, Value, Vm, VmConfig};

/// Runs on a thread with the main thread's stack: a deep script needs it.
fn run(source: &str, config: VmConfig) -> Value {
    let source = source.to_string();
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let mut vm = Vm::new(config).unwrap();
            vm.execute(&compile(&parse(&source).expect(&source)).expect(&source))
                .unwrap_or_else(|e| panic!("{source}: {e:?}"))
        })
        .unwrap()
        .join()
        .unwrap()
}

fn number(value: Value) -> f64 {
    match value {
        Value::Number(n) => n,
        other => panic!("{other:?}"),
    }
}

/// A document with `rows` objects of a few properties and a nested array each.
/// `JSON.parse` called from deep inside a framework's call stack: every active frame's
/// bindings are roots, and a bundle's functions each hold many.
const DEEP_PARSE: &str = "function deep(n, text) { var a0=0,a1=1,a2=2,a3=3,a4=4,a5=5,a6=6,a7=7,a8=8,a9=9,b0=0,b1=1,b2=2,b3=3,b4=4,b5=5,b6=6,b7=7,b8=8,b9=9,c0=0,c1=1,c2=2,c3=3,c4=4,c5=5,c6=6,c7=7,c8=8,c9=9; if (n > 0) { const r = deep(n - 1, text); return a0 + b9 + c4 === 13 ? r : null; } return JSON.parse(text); }";

const BUILD: &str = "const ROWS = COUNT; const rows = []; for (let i = 0; i < ROWS; i++) rows.push({ id: i, name: 'row ' + i, tags: ['a', 'b', i], nested: { x: i, y: [i, i + 1] } }); const text = JSON.stringify(rows);";

#[test]
fn parsing_a_large_document_costs_what_it_builds() {
    // Only the parse is timed, inside the script; the heap is sized as the page host's is.
    let source = format!(
        "{DEEP_PARSE} {} const started = Date.now(); const parsed = deep(100, text); const took = Date.now() - started; (parsed.length * 1000 + parsed[ROWS - 1].nested.y[1]) + ':' + took;",
        BUILD.replace("COUNT", "6000")
    );
    let result = run(
        &source,
        VmConfig {
            heap: HeapConfig {
                max_heap_bytes: 1024 * 1024 * 1024,
                nursery_capacity: 16_384,
                major_threshold_bytes: 16 * 1024 * 1024,
            },
            max_string_bytes: 64 * 1024 * 1024,
            instruction_budget: 2_000_000_000,
            ..VmConfig::default()
        },
    );
    let Value::String(text) = result else {
        panic!("{result:?}")
    };
    let text = text.to_utf8().unwrap();
    let (sum, took) = text.split_once(':').unwrap();
    assert_eq!(sum, (6000.0f64 * 1000.0 + 6000.0).to_string());
    let took: f64 = took.parse().unwrap();
    eprintln!("JSON.parse of 6,000 rows at call depth 100: {took} ms");
    assert!(took < 2000.0, "parsing 6,000 rows took {took} ms");
}

#[test]
fn parsing_agrees_when_the_heap_collects_at_every_allocation() {
    let source = format!(
        "{DEEP_PARSE} {} const parsed = deep(20, text); let s = 0; for (const r of parsed) s += r.id + r.tags[2] + r.nested.y[1] + r.name.length; s;",
        BUILD.replace("COUNT", "300")
    );
    let relaxed = number(run(
        &source,
        VmConfig {
            heap: HeapConfig {
                nursery_capacity: 100_000,
                ..HeapConfig::default()
            },
            instruction_budget: 200_000_000,
            ..VmConfig::default()
        },
    ));
    for nursery in [1usize, 2, 5] {
        let value = number(run(
            &source,
            VmConfig {
                heap: HeapConfig {
                    nursery_capacity: nursery,
                    major_threshold_bytes: 2 * 1024,
                    ..HeapConfig::default()
                },
                instruction_budget: 200_000_000,
                ..VmConfig::default()
            },
        ));
        assert_eq!(value, relaxed, "nursery {nursery}");
    }
}
