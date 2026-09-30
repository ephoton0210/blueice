// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `delete` applied to an optional chain deletes the chain's final Reference,
//! or evaluates to `true` without running any later part of the chain when a
//! `?.` short-circuits.
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
fn delete_of_a_non_nullish_optional_member_deletes_the_property() {
    check(&[
        "var o = {x: 1, y: 2}; delete o?.x === true && !('x' in o) && o.y === 2",
        "var o = {x: 1}; delete o?.['x'] === true && o.x === undefined && !('x' in o)",
        "var o = {a: {b: 1, c: 2}}; delete o.a?.b === true && !('b' in o.a) && o.a.c === 2",
        "var o = {a: {b: {c: 1}}}; delete o?.a.b?.c === true && !('c' in o.a.b)",
        "var o = Object.freeze({x: 1}); delete o?.x === false && o.x === 1",
    ]);
}

#[test]
fn delete_of_a_short_circuited_chain_is_true_and_skips_the_rest() {
    check(&[
        "var o = null; delete o?.x === true",
        "var o = null; delete o?.x.y === true",
        "var o = undefined; delete o?.[missing] === true",
        "var o = null; delete o?.x[missing1 + 1] === true",
        "delete undefined ?.x[missing2 + 1] === true",
        "var o = {a: null}; delete o.a?.b.c === true",
    ]);
}

#[test]
fn strict_delete_of_an_unremovable_chain_target_still_throws() {
    check(&[
        "'use strict'; var o = Object.freeze({x: 1}); \
         try { delete o?.x; false } catch (e) { e instanceof TypeError }",
        "'use strict'; var o = null; delete o?.x === true",
    ]);
}

#[test]
fn delete_of_an_optional_call_result_is_true() {
    check(&[
        "var log = []; var o = {f() { log.push('f'); return 1; }}; \
         delete o.f?.() === true && log.join() === 'f'",
        "var o = null; delete o?.f() === true",
    ]);
}
