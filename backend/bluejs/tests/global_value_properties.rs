// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The global value properties `NaN`, `Infinity` and `undefined` are
//! non-writable and non-configurable: `delete` of the bare name answers false,
//! and a strict assignment to the bare name throws a TypeError (not a
//! ReferenceError, since the property does exist).

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
fn delete_of_the_global_value_properties_is_false() {
    assert_true(
        r#"(function() {
          if (delete NaN !== false || delete Infinity !== false || delete undefined !== false) return "delete answered true";
          if (typeof NaN !== "number" || Infinity !== 1 / 0 || undefined !== void 0) return "value changed";
          // Ordinary configurable globals can still be deleted, and unknown names answer true.
          globalThis.configurableGlobal = 1;
          if (delete configurableGlobal !== true || "configurableGlobal" in globalThis) return "configurable global";
          if (delete neverDefinedAnywhere !== true) return "unknown name";
          return true;
        })()"#,
    );
}

#[test]
fn strict_assignment_to_the_global_value_properties_throws_a_type_error() {
    assert_true(
        r#"(function() {
          var results = [];
          var attempts = [
            function() { "use strict"; NaN = 12; },
            function() { "use strict"; Infinity = 12; },
            function() { "use strict"; undefined = 12; },
            function() { "use strict"; undefined += 1; },
          ];
          for (var attempt of attempts) {
            try { attempt(); results.push("no throw"); } catch (e) { results.push(e instanceof TypeError ? "TypeError" : e.name); }
          }
          if (results.join() !== "TypeError,TypeError,TypeError,TypeError") return results.join();
          // Sloppy assignment is silently ignored.
          NaN = 1; undefined = 2; Infinity = 3;
          if (NaN === NaN || undefined !== void 0 || Infinity !== 1 / 0) return "sloppy assignment changed a value";
          return true;
        })()"#,
    );
}
