// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `JSON.parse` revivers and `JSON.stringify` replacers run while the object
//! they are visiting changes. Both algorithms collect the enumerable own keys
//! up front (EnumerableOwnProperties), and the reviver's `[[Delete]]` /
//! CreateDataProperty results are ignored rather than turned into a TypeError.

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
fn a_reviver_cannot_break_the_walk_with_non_configurable_properties() {
    assert_true(
        r#"(function() {
          var arr = JSON.parse("[1, 2]", function(key, value) {
            if (key === "0") Object.defineProperty(this, "1", { configurable: false });
            if (key === "1") return 22;
            return value;
          });
          if (arr[0] !== 1 || arr[1] !== 2) return "create: " + arr.join();
          var kept = JSON.parse("[1, 2]", function(key, value) {
            if (key === "0") Object.defineProperty(this, "1", { configurable: false });
            if (key === "1") return undefined;
            return value;
          });
          if (!kept.hasOwnProperty("1") || kept[1] !== 2) return "delete";
          var obj = JSON.parse('{"a": 1, "b": 2}', function(key, value) {
            if (key === "a") Object.defineProperty(this, "b", { configurable: false });
            if (key === "b") return undefined;
            return value;
          });
          if (obj.b !== 2) return "object delete";
          // A frozen holder: neither define nor delete may throw.
          var frozen = JSON.parse('{"a": 1}', function(key, value) {
            if (key === "a") Object.freeze(this);
            return key === "a" ? 5 : value;
          });
          if (frozen.a !== 1) return "frozen";
          return true;
        })()"#,
    );
}

#[test]
fn a_reviver_still_visits_a_property_it_deleted_earlier() {
    assert_true(
        r#"(function() {
          Object.prototype.b = 3;
          try {
            var seen = [];
            var obj = JSON.parse('{"a": 1, "b": 2}', function(key, value) {
              seen.push(key + "=" + value);
              if (key === "a") delete this.b;
              return value;
            });
            if (seen.join() !== "a=1,b=3,=[object Object]") return "visited " + seen.join();
            if (!obj.hasOwnProperty("b") || obj.b !== 3) return "b was not recreated from the prototype";
          } finally { delete Object.prototype.b; }
          return true;
        })()"#,
    );
}

#[test]
fn a_replacer_that_deletes_a_later_property_still_serializes_its_key() {
    assert_true(
        r#"(function() {
          var obj = { get a() { delete this.b; return 1; }, b: 2 };
          var seenValue = "unset";
          var text = JSON.stringify(obj, function(key, value) {
            if (key === "b") { seenValue = value; return "<replaced>"; }
            return value;
          });
          if (text !== '{"a":1,"b":"<replaced>"}') return text;
          if (seenValue !== undefined) return "replacer saw " + seenValue;
          // Without a replacer the deleted property serializes as undefined and is omitted.
          var plain = JSON.stringify({ get a() { delete this.b; return 1; }, b: 2 });
          if (plain !== '{"a":1}') return plain;
          return true;
        })()"#,
    );
}
