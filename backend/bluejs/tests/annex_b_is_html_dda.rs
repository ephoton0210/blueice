// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Test262 `$262.IsHTMLDDA` host object is callable: Test262's
//! INTERPRETING.md requires it to return `null` when called with no argument
//! or with the empty String, so that abstract operations which look a method
//! up with `GetMethod` (which only rejects undefined/null, not a `typeof`
//! "undefined" object) can be observed calling it.

use blueice_bluejs::{compile, parse, Value, Vm};

fn check(source: &str) {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_is_html_dda().unwrap();
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true),
        "{source}"
    );
}

#[test]
fn the_host_object_is_callable_and_returns_null_for_the_documented_arguments() {
    check("$262.IsHTMLDDA()===null&&$262.IsHTMLDDA('')===null");
    check("typeof $262.IsHTMLDDA==='undefined'&&!$262.IsHTMLDDA&&$262.IsHTMLDDA==null");
    check("(function(){return this})()!==$262.IsHTMLDDA&&Function.prototype.call.call($262.IsHTMLDDA,undefined)===null");
}

#[test]
fn string_pattern_methods_call_an_is_html_dda_protocol_method() {
    for (method, symbol, arguments) in [
        ("match", "match", ""),
        ("matchAll", "matchAll", ""),
        ("replace", "replace", ""),
        ("replaceAll", "replace", ""),
        ("search", "search", ""),
        ("split", "split", ""),
    ] {
        // matchAll and replaceAll additionally require a global flag when the
        // argument is a RegExp (IsRegExp); an IsHTMLDDA object has no
        // Symbol.match, so that check is skipped.
        let source = format!(
            "var object=$262.IsHTMLDDA;var gets=0;\
             Object.defineProperty(object,Symbol.{symbol},{{get:function(){{gets++;return object}},configurable:true}});\
             \"\".{method}(object{arguments})===null&&gets===1"
        );
        check(&source);
    }
}
