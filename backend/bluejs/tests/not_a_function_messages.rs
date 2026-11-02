// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Calling something that is not a function names what was called (`a.b.c is not a function`),
//! as the engines pages are written against do: a page's crash is only diagnosable when the
//! message says which call failed.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn message(source: &str) -> String {
    let wrapped = format!("(function () {{ try {{ {source} }} catch (e) {{ return e instanceof TypeError ? e.message : 'not a TypeError: ' + e; }} return 'no error'; }})()");
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    match vm
        .execute(&compile(&parse(&wrapped).expect(source)).expect(source))
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
    {
        Value::String(text) => text.to_utf8().unwrap(),
        other => panic!("{source}: {other:?}"),
    }
}

#[test]
fn a_call_through_a_name_or_a_member_chain_is_named() {
    assert_eq!(message("let f; f();"), "f is not a function");
    assert_eq!(message("let f = 3; f();"), "f is not a function");
    assert_eq!(message("let o = {}; o.missing();"), "o.missing is not a function");
    assert_eq!(
        message("let a = { b: {} }; a.b.c();"),
        "a.b.c is not a function"
    );
    assert_eq!(message("let o = { n: 1 }; o.n();"), "o.n is not a function");
    assert_eq!(message("this.nothing();"), "this.nothing is not a function");
}

#[test]
fn computed_members_and_other_callees_get_a_generic_description() {
    assert_eq!(
        message("let a = [1]; a[0]();"),
        "a[...] is not a function"
    );
    assert_eq!(
        message("let g = () => 1; g()();"),
        "(intermediate value) is not a function"
    );
    assert_eq!(
        message("(void 0)();"),
        "(intermediate value) is not a function"
    );
}

#[test]
fn a_spread_call_and_an_optional_call_are_named_too() {
    assert_eq!(message("let f; f(...[1, 2]);"), "f is not a function");
    assert_eq!(message("let o = {}; o.m?.x.y();"), "no error");
    assert_eq!(
        message("let o = { m: {} }; o.m.x?.y; o.m.z();"),
        "o.m.z is not a function"
    );
}

#[test]
fn a_callback_a_builtin_rejects_is_not_blamed_on_the_builtin() {
    // `map` is a function; it is the callback that is not. The message must not say
    // "arr.map is not a function".
    let text = message("[1].map(undefined);");
    assert!(!text.contains("arr.map") && !text.contains("map is not a function"), "{text}");
    assert!(text.contains("callable"), "{text}");
}

#[test]
fn it_is_still_an_ordinary_catchable_type_error_and_a_working_call_is_untouched() {
    assert_eq!(message("let f = () => 1; return String(f());"), "1");
    assert_eq!(
        message("try { null(); } catch (e) { return e.name + ':' + (e instanceof TypeError); }"),
        "TypeError:true"
    );
}
