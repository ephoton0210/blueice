// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An ordinary function gets an `arguments` object only when something in it
//! can observe one. Building the exotic object costs several property
//! definitions, a heap cell per simple parameter (sloppy mapped arguments) and
//! a handful of intrinsic lookups on every call, and that made every
//! user-defined call an order of magnitude slower than a native one.
use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn run(source: &str) -> Result<Value, RuntimeError> {
    let program = parse(source).map_err(|error| RuntimeError::SyntaxError(error.message))?;
    let code = compile(&program).map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
    Vm::default().execute(&code)
}

/// Every construct that can name or reach the enclosing function's
/// `arguments` object, each in sloppy and strict form: eliding the object
/// must never change any of these results.
#[test]
fn every_way_of_reaching_arguments_still_sees_the_object() {
    let cases = [
        ("function f(a, b) { return arguments.length; } f(1, 2, 3)", 3.0),
        ("function f(a) { return arguments[0]; } f(7)", 7.0),
        ("function f(a) { return typeof arguments; } f() === 'object' ? 1 : 0", 1.0),
        ("function f() { return (() => arguments.length)(); } f(1, 2)", 2.0),
        ("function f() { return (() => (() => arguments[1])())(); } f(5, 6)", 6.0),
        ("function f() { return (x = arguments.length) => x; } f(1, 2, 3)()", 3.0),
        ("function f(x = arguments.length) { return x; } f(undefined, 2)", 2.0),
        ("function f({ n = arguments.length }) { return n; } f({}, 1, 2)", 3.0),
        ("function f() { return `${arguments.length}`; } f(1) === '1' ? 1 : 0", 1.0),
        ("function f() { return { arguments }.arguments.length; } f(1, 2)", 2.0),
        ("function f() { return [...arguments].length; } f(1, 2, 3)", 3.0),
        ("function f() { var [a, b] = arguments; return b; } f(1, 9)", 9.0),
        ("function f() { let x; [x] = arguments; return x; } f(4)", 4.0),
        ("function f() { return eval('arguments.length'); } f(1, 2, 3, 4)", 4.0),
        ("function f() { return (() => eval('arguments.length'))(); } f(1, 2)", 2.0),
        ("function f() { return eval('(() => arguments.length)()'); } f(1)", 1.0),
        ("function f() { with ({}) { return arguments.length; } } f(1, 2)", 2.0),
        ("function f() { return delete arguments; } f() ? 0 : 1", 1.0),
        ("function f() { arguments = 5; return arguments; } f()", 5.0),
        ("function f() { arguments++; return arguments; } f()", f64::NAN),
        (
            "function f() { return arguments.callee === f ? 1 : 0; } f()",
            1.0,
        ),
        ("function f() { return arguments; } f(1, 2).length", 2.0),
        ("class C { m() { return arguments.length; } } new C().m(1, 2)", 2.0),
        (
            "class C { constructor() { this.n = arguments.length; } } new C(1, 2, 3).n",
            3.0,
        ),
        ("function* g() { yield arguments.length; } g(1, 2).next().value", 2.0),
        (
            "async function f() { return arguments.length; } let n; f(1, 2).then(v => n = v); 1",
            1.0,
        ),
        ("({ m() { return arguments.length; } }).m(1, 2, 3)", 3.0),
        ("({ get p() { return arguments.length; } }).p", 0.0),
        ("function f() { return function() { return arguments.length; }(1, 2); } f(9)", 2.0),
        // Direct eval defines and reads `arguments` even when the function body
        // never spells the name.
        ("function f() { eval('var y = arguments[0]'); return y; } f(6)", 6.0),
        // Nothing here mentions `arguments` for `f`: the inner function's own
        // object must not be confused with it.
        (
            "function f() { function g() { return arguments.length; } return g(1, 2, 3); } f()",
            3.0,
        ),
        // A parameter or a lexical declaration that shadows the name.
        ("function f(arguments) { return arguments; } f(3)", 3.0),
        ("function f() { let arguments = 4; return arguments; } f()", 4.0),
        ("function f() { var arguments = 4; return arguments; } f(1, 2)", 4.0),
        ("function f() { var arguments; return typeof arguments; } f() === 'object' ? 1 : 0", 1.0),
        ("function f() { function arguments() {} return typeof arguments; } f() === 'function' ? 1 : 0", 1.0),
    ];
    for (source, expected) in cases {
        for strict in [false, true] {
            let program = if strict {
                format!("'use strict'; {source}")
            } else {
                source.to_string()
            };
            // Sloppy-only forms (`with`, assignment to `arguments`, ...) are
            // syntax errors in strict mode; only the sloppy run applies.
            let Ok(value) = run(&program) else {
                assert!(strict, "sloppy run failed: {source}");
                continue;
            };
            let matches = match (&value, expected) {
                (Value::Number(number), expected) if expected.is_nan() => number.is_nan(),
                (Value::Number(number), expected) => *number == expected,
                _ => false,
            };
            assert!(matches, "{program}: {value:?}, expected {expected}");
        }
    }
}

/// Sloppy-mode `arguments` aliases simple parameters (each captured into a
/// shared cell) and strict-mode `arguments` does not; neither may change when
/// the object is elided for functions that do not observe it.
#[test]
fn mapped_arguments_alias_parameters_only_in_sloppy_mode() {
    for (source, sloppy, strict) in [
        (
            "function f(a) { arguments[0] = 9; return a; } f(1)",
            9.0,
            1.0,
        ),
        (
            "function f(a) { a = 8; return arguments[0]; } f(1)",
            8.0,
            1.0,
        ),
        (
            "function f(a, b) { arguments[1] = 5; return a + b; } f(1, 2)",
            6.0,
            3.0,
        ),
        (
            "function f(a) { return (() => { arguments[0] = 4; })() || a; } f(1)",
            4.0,
            1.0,
        ),
        (
            "function f(a = 0) { arguments[0] = 9; return a; } f(1)",
            1.0,
            1.0,
        ),
    ] {
        for (program, expected) in [
            (source.to_string(), sloppy),
            (format!("'use strict'; {source}"), strict),
        ] {
            assert_eq!(run(&program), Ok(Value::Number(expected)), "{program}");
        }
    }
}

/// The point of the optimisation: a function that neither mentions
/// `arguments` nor contains a direct eval must not allocate anything per
/// call. Each avoided object also avoids collector work, so count collections
/// rather than time: 20,000 calls of three-parameter functions used to
/// allocate an arguments object plus a cell per parameter every time (several
/// hundred minor collections), and now allocate nothing. (The loops use `var`:
/// a per-iteration `let` binding allocates a cell of its own.)
#[test]
fn a_function_that_never_reads_arguments_allocates_nothing_per_call() {
    let sources = [
        "function f(a, b, c) { return a; } var n = 0; for (var i = 0; i < 20000; i++) { n += f(1, 2, 3); } n",
        "'use strict'; function f(a, b, c) { return a; } var n = 0; for (var i = 0; i < 20000; i++) { n += f(1, 2, 3); } n",
        "function f(a = 1, b = 2, c = 3) { return a; } var n = 0; for (var i = 0; i < 20000; i++) { n += f(1, 2, 3); } n",
        "var o = { m(a, b, c) { return a; } }; var n = 0; for (var i = 0; i < 20000; i++) { n += o.m(1, 2, 3); } n",
        "class C { m(a, b, c) { return a; } } var c = new C(); var n = 0; for (var i = 0; i < 20000; i++) { n += c.m(1, 2, 3); } n",
        // An inner function's own `arguments` is not this function's, and an
        // arrow that never names it does not need one either.
        "function inner() { return arguments.length; } function f(a, b, c) { return inner(a) + (a > 5 ? (() => a)() : 0); } var n = 0; for (var i = 0; i < 20000; i++) { n += f(1, 2, 3); } n",
    ];
    let config = || VmConfig {
        instruction_budget: 100_000_000,
        ..VmConfig::default()
    };
    for source in sources {
        let mut vm = Vm::new(config()).unwrap();
        let before = vm.heap().stats().minor_collections;
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap();
        let collections = vm.heap().stats().minor_collections - before;
        assert!(
            collections < 20,
            "{collections} minor collections for 20,000 calls of {source}"
        );
    }
    // A function that does use it keeps paying for it (and keeps working).
    let mut vm = Vm::new(config()).unwrap();
    let source = "function f(a, b, c) { return arguments.length; } var n = 0; for (var i = 0; i < 2000; i++) { n += f(1, 2, 3); } n";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap()),
        Ok(Value::Number(6000.0))
    );
    assert!(vm.heap().stats().minor_collections > 20);
}
