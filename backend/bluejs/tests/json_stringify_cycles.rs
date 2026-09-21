// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON.stringify keeps its own cycle-detection stack per call: neither a
//! nested call made from a replacer nor an Array.prototype.join in progress
//! may make an unrelated value look cyclic.
use blueice_bluejs::{compile, parse, Value, Vm};

fn check(sources: &[&str]) {
    for source in sources {
        let value = Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(value, Value::Bool(true), "{source}");
    }
}

#[test]
fn nested_stringify_of_an_enclosing_value_is_not_a_cycle() {
    check(&[
        "var arr = [{}]; var inner; \
         var outer = JSON.stringify(arr, function (k, v) { inner = JSON.stringify(arr); return v; }); \
         outer === '[{}]' && inner === '[{}]'",
    ]);
}

#[test]
fn stringify_inside_an_array_join_is_not_a_cycle() {
    check(&[
        "var arr = [{ toString: function () { return JSON.stringify(arr); } }]; \
         arr.join() === '[{}]'",
    ]);
}

#[test]
fn real_cycles_are_still_rejected() {
    check(&[
        "var a = {}; a.self = a; try { JSON.stringify(a); false } catch (e) { e instanceof TypeError }",
        "var a = []; a[0] = a; try { JSON.stringify(a); false } catch (e) { e instanceof TypeError }",
        "var a = {}; var b = {a: a}; a.b = b; \
         try { JSON.stringify({x: a}); false } catch (e) { e instanceof TypeError }",
        // The same object twice, but not nested inside itself, is fine.
        "var shared = {v: 1}; JSON.stringify([shared, shared]) === '[{\"v\":1},{\"v\":1}]'",
    ]);
}
