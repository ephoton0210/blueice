// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every byte and metadata boundary in representative expression shapes must
//! either produce complete code units or fail before returning partial bytecode.

use blueice_bluejs::{
    compile, compile_module, compile_module_with_limit, compile_with_limit, compile_with_limits,
    parse, parse_module, Bytecode, CompileError, CompileLimits, RuntimeError, Value, Vm, VmConfig,
};

fn total_compiled_bytes(code: &Bytecode) -> usize {
    code.bytes().len()
        + code
            .child_code_units()
            .map(total_compiled_bytes)
            .sum::<usize>()
}

fn all_code_bytes(code: &Bytecode) -> Vec<Vec<u8>> {
    let mut units = vec![code.bytes().to_vec()];
    for child in code.child_code_units() {
        units.extend(all_code_bytes(child));
    }
    units
}

const EXPRESSION_CASES: &[&str] = &[
        "function tag(strings) {} tag`raw`; tag`a${1}b`;",
        "({ tag(strings) {} }).tag`raw`;",
        "class C { #tag(strings) {} run() { return this.#tag`raw`; } }",
        "class A { tag(strings) {} } class B extends A { run() { return super.tag`raw`; } }",
        "let x = 1; void x; +x; -x; ~x; !x; typeof x; delete x;",
        "/a/gi; 2n; undefined; NaN; Infinity; unknown; typeof unknown;",
        "let x = {}; delete x.a; delete x['a']; delete x?.a.b;",
        "let x = null; delete x?.a?.b; delete missing; delete (1 + 2);",
        "with ({ x: 1 }) { typeof x; typeof missing; delete x; x++; x += 2; }",
        "with ({}) { let local = 1; local; }",
        "with ({ x: 1, f() {} }) { f(); missing; missing++; delete missing; }",
        "with ({ x: 1 }) { x &&= 2; x ||= 3; x ??= 4; missing &&= 5; }",
        "let x = [1, , ...[2], 3]; x;",
        "({ a: 1, ['b']: function() {}, get c() { return 2; }, set c(v) {}, m() {}, ...{ d: 4 } });",
        "({ __proto__: null, a: 1, ['x']: class {} });",
        "({ ['x']() {}, get ['y']() { return 1; }, set ['y'](v) {}, 'a': 1, 2: 3 });",
        "function f(...x) {} f(1, 2); f(...[3]); new f(1); new f(...[2]);",
        "eval('1'); eval(...['2']);",
        "let x = { f() {} }; x.f(); (x.f)(); x?.f(); x.f?.(); (x?.f)();",
        "let x = null; x?.a?.b; x?.[1]?.(2); x?.a(1, ...[2]);",
        "let x = null; x?.a.b(1); (x?.a)(); x?.a?.(1, ...[2]);",
        "let f = function() { return function() {}; }; f?.(); f?.()();",
        "let x = { m() {} }; (x.m)?.(); (x?.m)?.();",
        "let x = {}; x.a++; ++x.a; x['b']--; --x['b'];",
        "let x = 1; x++; --x; x += 2; x &&= 3; x ||= 4; x ??= 5;",
        "missing++;",
        "let x = {}; x.a = 1; x.a += 2; x.a &&= 3; x.a ||= 4; x.a ??= 5;",
        "let x = 1; x ? x : 0; x && 2; x || 3; x ?? 4; (x, 2);",
        "let x = `a${1}b`; x;",
        "let x = () => 1; let y = async (a) => { return await a; };",
        "function* g() { yield; yield 1; yield* [2]; }",
        "async function* g() { yield; yield 1; yield* [2]; }",
        "class A {} class B extends A { constructor() { super(); } }",
        "class A {} class B extends A { constructor() { super(...[]); } }",
        "class A { m() {} } class B extends A { m() { super.m(); return super.m?.(); } }",
        "class A { m() {} } class B extends A { m() { return (super.m)(); } }",
        "class A { m() {} } class B extends A { m() { return super.m?.x; } }",
        "class A { get x() { return 1; } set x(v) {} } class B extends A { m() { super.x; super.x = 2; super.x += 3; super.x &&= 4; super.x++; delete super.x; } }",
        "class A {} class B extends A { constructor() { delete super.x; delete super[1]; super(); } }",
        "class C { #x = 1; m() { this.#x; this.#x = 2; this.#x += 3; this.#x ??= 4; this.#x++; return #x in this; } }",
        "class C { #x = 1; m() { return this?.a.#x; } }",
        "class C { #m() {} run() { return this?.a.#m?.(); } }",
        "class C { #m() {} run() { return (this.#m)(); } }",
        "class C { #x = 1; #m() {} run() { this?.#x; this?.#m?.(); return (this.#m)?.(); } }",
        "for (let x in { a: 1 }) { x; } for (var x of [1]) { x; }",
        "async function f() { for await (let x of [1]) { x; } }",
        "async function f() { for await (await using x of [null]) { x; } }",
        "for (var x = 1 in { a: 2 }) { x; }",
        "let x; for (x of [1]) { x; } for (x in { a: 1 }) { x; }",
        "for (missing of [1]) {}",
        "let target = {}; for (target.x of [1]) {} for (target.y in { a: 1 }) {}",
        "class A { set x(value) {} } class B extends A { constructor() { super(); for (super.x of [1]) {} this.value = 2; } }",
        "class C { #x; run() { for (this.#x of [1]) {} } }",
        "function f() {} for (f() in { a: 1 }) {}",
        "let a, b; [a, , b = 2] = [1]; ({ x: a = 3, ...b } = { x: 4 });",
        "let target = {}; [target.x, ...target.y] = [1, 2]; ({ x: target.z } = { x: 3 });",
        "let a; [a, ...a] = [1, 2]; ({ x: a, ...a } = { x: 3 });",
        "class A { set x(v) {} } class B extends A { m() { [super.x] = [1]; ({ x: super.x } = { x: 2 }); } }",
        "class C { #x; m() { [this.#x] = [1]; ({ x: this.#x } = { x: 2 }); } }",
        "let x = {}; (x.a = function() {}); (x['b'] = class {});",
        "missing = 1; missing += 2; missing &&= 3; missing ||= 4; missing ??= 5;",
        "'use strict'; missing = 1;",
        "function f() { return new.target; }",
        "function f() {} f() = 1;",
        "import('m'); import('m', { with: { type: 'json' } });",
];

#[test]
fn expression_families_reject_every_truncated_bytecode_budget() {
    for &source in EXPRESSION_CASES {
        let program = parse(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        let full = compile(&program).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        let expected = all_code_bytes(&full);
        let upper = u32::try_from(total_compiled_bytes(&full)).unwrap();
        let mut first_success = None;
        for limit in 0..=upper {
            match compile_with_limit(&program, limit) {
                Ok(code) => {
                    first_success.get_or_insert(limit);
                    assert_eq!(all_code_bytes(&code), expected, "{source} at {limit} bytes");
                }
                Err(CompileError::ProgramTooLarge) => {
                    assert!(first_success.is_none(), "{source} at {limit} bytes");
                }
                Err(error) => panic!("{source} at {limit} bytes: {error:?}"),
            }
        }
        assert!(first_success.is_some(), "{source}");
    }
}

#[test]
fn expression_families_reject_every_truncated_metadata_budget() {
    for &source in EXPRESSION_CASES {
        let program = parse(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        let full = compile(&program).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        let expected = all_code_bytes(&full);
        let upper = source.len() as u64 + total_compiled_bytes(&full) as u64 + 1;
        let mut first_success = None;
        for limit in 0..=upper {
            let limits = CompileLimits {
                max_metadata_entries: limit,
                ..CompileLimits::default()
            };
            match compile_with_limits(&program, limits) {
                Ok(code) => {
                    first_success.get_or_insert(limit);
                    assert_eq!(
                        all_code_bytes(&code),
                        expected,
                        "{source} at {limit} entries"
                    );
                    break;
                }
                Err(CompileError::ProgramTooLarge) => {
                    assert!(first_success.is_none(), "{source} at {limit} entries");
                }
                Err(error) => panic!("{source} at {limit} entries: {error:?}"),
            }
        }
        assert!(first_success.is_some(), "{source}");
    }
}

#[test]
fn direct_eval_var_deletion_rejects_each_truncated_bytecode_budget() {
    let outer = compile(&parse("eval('var x = 1; delete x; typeof x;');").unwrap()).unwrap();
    let mut first_success = None;
    for limit in 0..=512 {
        let mut vm = Vm::new(VmConfig {
            eval_compile_limits: CompileLimits {
                max_bytecode_bytes: limit,
                ..CompileLimits::default()
            },
            ..VmConfig::default()
        })
        .unwrap();
        match vm.execute(&outer) {
            Ok(Value::String(value)) if value == "undefined" => {
                first_success = Some(limit);
                break;
            }
            Err(RuntimeError::SyntaxError(message)) if message.contains("limit") => {}
            result => panic!("unexpected eval result at {limit} bytes: {result:?}"),
        }
    }
    assert!(first_success.is_some());
}

#[test]
fn import_meta_rejects_every_truncated_module_bytecode_budget() {
    let module = parse_module("import.meta;").unwrap();
    let full = compile_module(&module).unwrap();
    let expected = all_code_bytes(&full);
    let upper = u32::try_from(total_compiled_bytes(&full)).unwrap();
    let mut first_success = None;
    for limit in 0..=upper {
        match compile_module_with_limit(&module, limit) {
            Ok(code) => {
                first_success.get_or_insert(limit);
                assert_eq!(all_code_bytes(&code), expected);
            }
            Err(CompileError::ProgramTooLarge) => assert!(first_success.is_none()),
            Err(error) => panic!("unexpected module error at {limit} bytes: {error:?}"),
        }
    }
    assert!(first_success.is_some());
}

#[test]
fn list_item_limits_reject_oversized_arrays_and_argument_lists() {
    for source in [
        "let x = [1, 2];",
        "let x = [, 1];",
        "let x = [...[], 1];",
        "function f() {} f(1, 2);",
        "function f() {} f(1, ...[2]);",
        "function F() {} new F(1, 2);",
        "let f = () => 0; f?.(1, 2);",
        "class A {} class B extends A { constructor() { super(1, 2); } }",
        "function tag() {} tag`a${1}b`;",
    ] {
        let program = parse(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        let expected = all_code_bytes(&compile(&program).unwrap());
        for limit in 0..=2 {
            let result = compile_with_limits(
                &program,
                CompileLimits {
                    max_list_items: limit,
                    ..CompileLimits::default()
                },
            );
            if limit < 2 {
                assert!(
                    matches!(result, Err(CompileError::ProgramTooLarge)),
                    "{source} at {limit} items"
                );
            } else {
                assert_eq!(
                    all_code_bytes(&result.unwrap()),
                    expected,
                    "{source} at {limit} items"
                );
            }
        }
    }
}
