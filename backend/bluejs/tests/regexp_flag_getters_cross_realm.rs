// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The RegExp flag and `source` getters treat a RegExp of another Test262
//! realm as a RegExp (it has [[OriginalFlags]]) and that realm's own
//! %RegExp.prototype% as an ordinary non-RegExp object, so it throws a
//! TypeError instead of answering undefined.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

#[test]
fn regexp_flag_getters_handle_foreign_regexps_and_prototypes() {
    assert_true(
        r#"(function() {
          var other = $262.createRealm().global;
          var dotAll = Object.getOwnPropertyDescriptor(RegExp.prototype, "dotAll").get;
          var source = Object.getOwnPropertyDescriptor(RegExp.prototype, "source").get;
          var foreign = new other.RegExp("a/b", "gs");
          if (dotAll.call(foreign) !== true) return "flag of a foreign RegExp";
          if (Object.getOwnPropertyDescriptor(RegExp.prototype, "global").get.call(foreign) !== true) return "global flag";
          if (Object.getOwnPropertyDescriptor(RegExp.prototype, "sticky").get.call(foreign) !== false) return "sticky flag";
          if (source.call(foreign) !== "a\\/b") return "source of a foreign RegExp: " + source.call(foreign);
          // The other realm's %RegExp.prototype% is not this realm's, so it is just a non-RegExp object.
          try { dotAll.call(other.RegExp.prototype); return "foreign prototype accepted"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          var otherDotAll = Object.getOwnPropertyDescriptor(other.RegExp.prototype, "dotAll").get;
          try { otherDotAll.call(RegExp.prototype); return "local prototype accepted by the other realm"; }
          catch (e) { if (!(e instanceof other.TypeError)) return "wrong realm for the error"; }
          // A realm's own prototype still answers undefined / "(?:)".
          if (dotAll.call(RegExp.prototype) !== undefined || source.call(RegExp.prototype) !== "(?:)") return "own prototype";
          return true;
        })()"#,
    );
}
