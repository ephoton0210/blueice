// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Date` through the public VM: string parsing (ISO, the spaced variant and
//! the formats `toString`/`toUTCString` print), constructor and `Date.UTC`
//! argument handling, getters, setters, conversions and the abrupt paths of
//! each. The expected strings were produced by Node with `TZ=UTC`, which
//! BlueJS's fixed UTC host time zone matches.

use blueice_bluejs::{compile, parse, Value, Vm};

/// Evaluates `expr` and reports its `String` conversion, or `throws Name`.
fn observe(expr: &str) -> String {
    let source = format!(
        "(function(){{ try {{ return String({expr}) }} catch (e) {{ return 'throws ' + e.constructor.name }} }})()"
    );
    let mut vm = Vm::default();
    match vm.execute(&compile(&parse(&source).unwrap()).unwrap()) {
        Ok(Value::String(text)) => text.to_utf8().unwrap(),
        other => format!("unexpected {other:?}"),
    }
}

#[test]
fn date_behaviour_matches_the_reference_results() {
    let mut mismatches = Vec::new();
    for (expr, expected) in DATE_CASES {
        let actual = observe(expr);
        if actual != *expected {
            mismatches.push(format!(
                "{expr}\n    expected {expected}\n    actual   {actual}"
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

include!("cov_g1_tables/date.in");
