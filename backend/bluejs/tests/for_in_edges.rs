// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `for-in` head evaluation (§14.7.5.6 ForIn/OfHeadEvaluation): a `null` or
//! `undefined` subject runs no iteration instead of throwing, and other
//! primitives are boxed.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap()
}

#[test]
fn for_in_over_null_or_undefined_runs_no_iterations() {
    for subject in ["null", "undefined", "void 0"] {
        assert_eq!(
            evaluate(&format!("var n = 0; for (var k in {subject}) n++; n")),
            Value::Number(0.0),
            "{subject}"
        );
        assert_eq!(
            evaluate(&format!("var n = 0; for (let k in {subject}) {{ n++; }} n")),
            Value::Number(0.0),
            "{subject}"
        );
        assert_eq!(
            evaluate(&format!("var n = 0; var k; for (k in {subject}) n++; n")),
            Value::Number(0.0),
            "{subject}"
        );
    }
}

#[test]
fn for_in_over_null_leaves_an_undefined_completion_value() {
    assert_eq!(
        evaluate("eval('1; for (var a in undefined) { }')"),
        Value::Undefined
    );
    assert_eq!(
        evaluate("eval('2; for (var b in null) { 3; }')"),
        Value::Undefined
    );
    assert_eq!(
        evaluate("eval('4; for (var c in null);')"),
        Value::Undefined
    );
}

#[test]
fn for_in_still_boxes_other_primitives() {
    assert_eq!(
        evaluate("var s = ''; for (var k in 'ab') s += k; s"),
        Value::String("01".into())
    );
    assert_eq!(
        evaluate("var n = 0; for (var k in 5) n++; n"),
        Value::Number(0.0)
    );
}

// EnumerateObjectProperties: "A property that is deleted before it is
// processed by the iterator's next method is ignored."

#[test]
fn for_in_skips_a_property_deleted_before_it_is_visited() {
    assert_eq!(
        evaluate(
            "var o = {a: 1, b: 2, c: 3}; var seen = [];
             for (var k in o) { delete o.b; seen.push(k); }
             seen.join()"
        ),
        Value::String("a,c".into())
    );
}

#[test]
fn for_in_skips_array_indexes_removed_by_a_length_change_or_shift() {
    assert_eq!(
        evaluate("var a = [0, 1], n = 0; for (var k in a) { n++; a.length = 1; } n"),
        Value::Number(1.0)
    );
    assert_eq!(
        evaluate(
            "var a = [1, , 3], keys = [];
             for (var k in a) { keys.push(k); if (keys.length == 1) a.unshift(0); }
             keys.join()"
        ),
        Value::String("0".into())
    );
    assert_eq!(
        evaluate(
            "var a = [0, 1, 2, 3, 4, 5, , 7], seen = [];
             for (var k in a) { if (k === '1') a.splice(2, 3); seen.push(k); }
             seen.indexOf('3')"
        ),
        Value::Number(-1.0)
    );
}

#[test]
fn for_in_skips_a_property_made_non_enumerable_before_it_is_visited() {
    assert_eq!(
        evaluate(
            "var o = {a: 1, b: 2}, seen = [];
             for (var k in o) {
                 Object.defineProperty(o, 'b', {enumerable: false});
                 seen.push(k);
             }
             seen.join()"
        ),
        Value::String("a".into())
    );
}

#[test]
fn for_in_does_not_visit_properties_added_during_the_loop() {
    assert_eq!(
        evaluate(
            "var o = {p1: 1, p2: 2}, seen = [];
             for (var k in o) { o.p3 = 3; seen.push(k); }
             seen.join()"
        ),
        Value::String("p1,p2".into())
    );
}

#[test]
fn for_in_ignores_user_changes_to_the_array_iterator_protocol() {
    // The enumeration is not observable through `Array.prototype[@@iterator]`.
    assert_eq!(
        evaluate(
            "var original = Array.prototype[Symbol.iterator];
             Array.prototype[Symbol.iterator] = function () { throw new Error('observed'); };
             var seen = [];
             try { for (var k in {a: 1, b: 2}) seen.push(k); }
             finally { Array.prototype[Symbol.iterator] = original; }
             seen.join()"
        ),
        Value::String("a,b".into())
    );
}

#[test]
fn for_in_keeps_working_across_break_continue_labels_and_generators() {
    assert_eq!(
        evaluate(
            "var o = {a: 1, b: 2, c: 3}, seen = [];
             outer: for (var k in o) {
                 for (var j in o) { if (j == 'b') continue outer; seen.push(k + j); }
             }
             for (var m in o) { if (m == 'b') break; seen.push(m); }
             seen.join()"
        ),
        Value::String("aa,ba,ca,a".into())
    );
    assert_eq!(
        evaluate(
            "function* g(o) { for (var k in o) yield k; }
             Array.from(g({x: 1, y: 2})).join()"
        ),
        Value::String("x,y".into())
    );
}
