// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A destructuring assignment target may be a parenthesized simple target,
//! with or without a default (`[(a) = 5] = []`, `({ k: (o.p) = 1 } = {})`);
//! a parenthesized pattern or a parenthesized element with a default is an
//! early error. Parentheses also switch off anonymous-function naming, since
//! `(a)` is not an IdentifierRef.
use blueice_bluejs::{compile, parse, Value, Vm};

fn is_rejected(source: &str) -> bool {
    match parse(source) {
        Err(error) => {
            assert!(error.known_syntax, "{source:?}: {error:?}");
            true
        }
        Ok(program) => compile(&program).is_err(),
    }
}

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    assert_eq!(
        Vm::default()
            .execute(&code)
            .unwrap_or_else(|e| panic!("{source}: {e:?}")),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn a_parenthesized_identifier_target_takes_a_default() {
    for source in [
        "var a, b; [(a) = 5, b] = [undefined, 2]; a === 5 && b === 2",
        "var a, b; [(a) = 5, b] = [1, 2]; a === 1 && b === 2",
        "var a, b; ({ a: (a) = 5, b } = { b: 2 }); a === 5 && b === 2",
        "var a, b; [((a)) = 5] = []; a === 5",
        "(function () { var a; [(arguments) = 5] = []; return arguments === 5; })()",
        "var eval; [(eval) = 5] = []; eval === 5",
    ] {
        assert_true(source);
    }
}

#[test]
fn a_parenthesized_member_target_takes_a_default() {
    for source in [
        "var o = {}; [(o.x) = 'motel'] = []; o.x === 'motel'",
        "var o = {}; [(o['x']) = 'motel', ...rest] = [undefined, 1]; o.x === 'motel' && rest[0] === 1",
        "var o = {}; var k = 'k'; [(o[k + {}]) = 1] = []; o['k[object Object]'] === 1",
        "var o = {}; ({ p: (o.q) = 3 } = {}); o.q === 3",
        "var o = { x() { var r = {}; [(super.man), r.b] = [1, 2]; return r.b === 2 && this.man === 1; } };
         Object.setPrototypeOf(o, {}); o.x()",
        "var o = { x() { [(super[8]) = 'motel'] = []; return this[8] === 'motel'; } }; o.x()",
        "var o = { x() { [(super[8 + {}]) = 'motel'] = []; return this['8[object Object]'] === 'motel'; } }; o.x()",
    ] {
        assert_true(source);
    }
}

#[test]
fn a_parenthesized_target_does_not_name_an_anonymous_default() {
    assert_true(
        "var a, b; [(a) = function () {}] = []; [b = function () {}] = [];
         a.name === '' && b.name === 'b'",
    );
}

#[test]
fn a_parenthesized_pattern_or_an_element_with_a_default_is_an_early_error() {
    for source in [
        "var a, b; ([a, b]) = [1, 2];",
        "var a, b; ({a, b}) = { a: 1, b: 2 };",
        "var a, b; ({ a: ({ b: b }) } = { a: { b: 42 } });",
        "var a, b; ({ a: { b: (b = 7) } } = { a: {} });",
        "var a, b; ({ a: ([b]) } = { a: [42] });",
        "var a, b; [(a = 5)] = [1];",
        "var a, b; ({ a: (b = 7)} = { b: 1 });",
        "var o = {}; [(o.man = 17)] = [1];",
        "var a, b; [f() = 'ohai', b] = [1, 2];",
        "'use strict'; var a, b; [(f()) = 'kthxbai', b] = [1, 2];",
    ] {
        assert!(is_rejected(source), "{source}");
    }
}
