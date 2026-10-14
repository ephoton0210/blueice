// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `parseInt` and `parseFloat` read the longest numeric prefix of a UTF-16
//! string; a lone surrogate (which has no UTF-8 form) just ends that prefix
//! instead of making the whole result NaN.

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let value = Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"));
    match value {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

#[test]
fn a_lone_surrogate_ends_the_numeric_prefix() {
    assert_true(
        r#"(function() {
          for (var unit of [0xd800, 0xdbff, 0xdc00, 0xdfff]) {
            var c = String.fromCharCode(unit);
            if (parseFloat("0.1e1" + c) !== 1) return "parseFloat " + unit.toString(16);
            if (parseFloat("12.5" + c + "7") !== 12.5) return "parseFloat fraction " + unit.toString(16);
            if (parseInt("1Z" + c, 36) !== 71) return "parseInt " + unit.toString(16);
            if (parseInt("42" + c + "9") !== 42) return "parseInt decimal " + unit.toString(16);
            if (parseInt(" \t7" + c) !== 7 || parseFloat(" \n-3.5" + c) !== -3.5) return "leading whitespace";
            if (!Number.isNaN(parseInt(c + "1")) || !Number.isNaN(parseFloat(c + "1"))) return "a leading surrogate is not a number";
          }
          // A valid surrogate pair behaves the same way.
          if (parseFloat("8😀") !== 8 || parseInt("8😀") !== 8) return "pair";
          return true;
        })()"#,
    );
}
