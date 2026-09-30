// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The order of the observable steps of the RegExp constructor and of
//! RegExpBuiltinExec: user code that runs while one of them is in progress
//! (a `prototype` getter, a `toString`, a `lastIndex` `valueOf`) sees the
//! state the specification says exists at that point.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
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
fn constructor_looks_up_the_prototype_before_it_stringifies_pattern_and_flags() {
    // RegExpAlloc (OrdinaryCreateFromConstructor, which reads
    // `newTarget.prototype`) precedes RegExpInitialize (ToString of both).
    assert_true(
        r#"(function() {
          var log = [];
          var target = Object.defineProperty(function() {}.bind(null), "prototype", {
            get() { log.push("prototype"); return RegExp.prototype; }
          });
          var pattern = { toString() { log.push("pattern"); return "a"; } };
          var flags = { toString() { log.push("flags"); return "g"; } };
          Reflect.construct(RegExp, [pattern, flags], target);
          if (log.join() !== "prototype,pattern,flags") return "plain pattern: " + log.join();
          // Same for a pattern with a [[RegExpMatcher]] slot and an explicit flags argument.
          log = [];
          var newRe = Reflect.construct(RegExp, [/a/, flags], target);
          if (log.join() !== "prototype,flags") return "RegExp pattern: " + log.join();
          if (newRe.flags !== "g") return "flags " + newRe.flags;
          // And for a pattern that is only regexp-like: `source` and `flags` are read first.
          log = [];
          var like = {
            get source() { log.push("source"); return "b"; },
            get flags() { log.push("flags getter"); return "i"; },
            [Symbol.match]: true,
            constructor: RegExp,
          };
          var fromLike = Reflect.construct(RegExp, [like], target);
          if (log.join() !== "source,flags getter,prototype") return "regexp-like: " + log.join();
          if (fromLike.source !== "b" || fromLike.flags !== "i") return "regexp-like result";
          return true;
        })()"#,
    );
}

#[test]
fn constructor_still_reports_a_bad_prototype_getter_before_a_bad_pattern() {
    assert_true(
        r#"(function() {
          var target = Object.defineProperty(function() {}.bind(null), "prototype", {
            get() { throw new RangeError("prototype"); }
          });
          try { Reflect.construct(RegExp, ["(", "g"], target); return "no error"; }
          catch (e) { return e instanceof RangeError; }
        })()"#,
    );
}

#[test]
fn exec_reads_flags_and_matcher_after_coercing_last_index() {
    // ToLength(lastIndex) can recompile the RegExp, and RegExpBuiltinExec
    // uses whatever [[RegExpMatcher]]/[[OriginalFlags]] it has afterwards.
    assert_true(
        r#"(function() {
          for (var flag of ["", "y"]) {
            var re = new RegExp("a", flag);
            re.lastIndex = { valueOf() { re.compile("b"); return 0; } };
            if (re.exec("b") === null) return "recompiled pattern ignored, flag " + flag;
          }
          // Adds the global flag: exec now records where the match ended.
          var added = new RegExp("a");
          added.lastIndex = { valueOf() { added.compile("a", "g"); return 0; } };
          added.exec("a");
          if (added.lastIndex !== 1) return "global flag ignored: " + added.lastIndex;
          // Drops the sticky flag: exec leaves lastIndex alone.
          var dropped = new RegExp("a", "y");
          dropped.lastIndex = { valueOf() { dropped.compile("a", ""); dropped.lastIndex = 9000; return 0; } };
          dropped.exec("a");
          if (dropped.lastIndex !== 9000) return "sticky flag ignored: " + dropped.lastIndex;
          return true;
        })()"#,
    );
}

#[test]
fn match_and_replace_observe_a_recompilation_during_last_index_coercion() {
    assert_true(
        r#"(function() {
          for (var flag of ["", "y"]) {
            var re = new RegExp("a", flag);
            re.lastIndex = { valueOf() { re.compile("b"); return 0; } };
            if (re[Symbol.match]("b") === null) return "match, flag " + flag;
            re = new RegExp("a", flag);
            re.lastIndex = { valueOf() { re.compile("b"); return 0; } };
            if (re[Symbol.replace]("b", "pass") !== "pass") return "replace, flag " + flag;
          }
          var re = new RegExp("a", "y");
          re.lastIndex = { valueOf() { re.compile("b", ""); re.lastIndex = 9002; return 10000; } };
          re[Symbol.replace]("a", "");
          if (re.lastIndex !== 9002) return "no match, sticky dropped: " + re.lastIndex;
          return true;
        })()"#,
    );
}
