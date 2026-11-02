// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reading a property of `null` or `undefined` names the property and the value, as the
//! engines pages are written against do (`Cannot read properties of null (reading 'x')`):
//! a page's crash is only diagnosable when the message says what was read.

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
fn a_read_names_the_property_and_the_value() {
    assert_eq!(
        message("null.alpha;"),
        "Cannot read properties of null (reading 'alpha')"
    );
    assert_eq!(
        message("let u; u.beta;"),
        "Cannot read properties of undefined (reading 'beta')"
    );
    assert_eq!(
        message("let o = {}; o.inner.gamma;"),
        "Cannot read properties of undefined (reading 'gamma')"
    );
}

#[test]
fn computed_numeric_and_symbol_keys_are_named() {
    assert_eq!(
        message("let a; a[3];"),
        "Cannot read properties of undefined (reading '3')"
    );
    assert_eq!(
        message("let a = null; a['two words'];"),
        "Cannot read properties of null (reading 'two words')"
    );
    assert_eq!(
        message("let a; a[Symbol('tag')];"),
        "Cannot read properties of undefined (reading 'Symbol(tag)')"
    );
}

#[test]
fn a_write_and_a_method_call_name_the_property_too() {
    // A write shares its reference preparation with compound assignment, which reads first, so
    // the verb is "read" for both; the property and the value are what a reader needs.
    let write = message("let a; a.delta = 1;");
    assert!(
        write.starts_with("Cannot read properties of undefined (") && write.contains("'delta'"),
        "{write}"
    );
    assert_eq!(
        message("let a = null; a.run();"),
        "Cannot read properties of null (reading 'run')"
    );
}

#[test]
fn a_very_long_key_is_cut_not_copied_whole() {
    let long = "k".repeat(500);
    let text = message(&format!("let a; a['{long}'];"));
    assert!(text.starts_with("Cannot read properties of undefined (reading '"));
    assert!(text.len() < 200, "{}", text.len());
}

#[test]
fn it_is_still_an_ordinary_catchable_type_error() {
    assert_eq!(
        message("try { null.x } catch (e) { return e.name + ':' + (e instanceof TypeError); }"),
        "TypeError:true"
    );
}
