// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A `return` inside a for-of body completes the loop abruptly: the
//! surrounding `try`/`catch`/`finally` blocks are unwound first (so a throwing
//! iterator `return()` is not catchable by a `try` inside the body and runs
//! after that body's finalizers), and the loop's iterator is closed last.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

const ITERABLE: &str = "
    var log = [];
    function iterable(name, returnBehavior) {
      var it = {};
      it[Symbol.iterator] = function () {
        return {
          next: function () { return { done: false, value: 0 }; },
          return: function () { log.push('close ' + name); return returnBehavior ? returnBehavior() : {}; }
        };
      };
      return it;
    }
";

#[test]
fn a_throwing_return_method_is_not_caught_by_a_try_inside_the_body() {
    let source = format!(
        "{ITERABLE}
         var catchEntered = 0, finallyEntered = 0, thrown;
         function f() {{
           for (var x of iterable('it', function () {{ throw 42; }})) {{
             try {{ return; }} catch (e) {{ catchEntered++; }} finally {{ finallyEntered++; }}
           }}
         }}
         try {{ f(); }} catch (e) {{ thrown = e; }}
         [thrown, catchEntered, finallyEntered, log.join()].join('|')"
    );
    assert_eq!(evaluate(&source), Value::String("42|0|1|close it".into()));
}

#[test]
fn the_iterator_is_closed_after_the_finalizers_of_the_body_run() {
    let source = format!(
        "{ITERABLE}
         function f() {{
           for (var x of iterable('outer')) {{
             try {{
               for (var y of iterable('inner')) {{ try {{ return 7; }} finally {{ log.push('finally innermost'); }} }}
             }} finally {{ log.push('finally middle'); }}
           }}
         }}
         var r = f();
         r + '|' + log.join()"
    );
    // The innermost try sits inside the inner loop, so the inner iterator is
    // closed only after it; the middle finalizer runs before the outer loop
    // (which surrounds it) is closed.
    assert_eq!(
        evaluate(&source),
        Value::String("7|finally innermost,close inner,finally middle,close outer".into())
    );
}

#[test]
fn a_return_without_a_try_closes_every_loop_innermost_first() {
    let source = format!(
        "{ITERABLE}
         function f() {{
           for (var x of iterable('a')) for (var y of iterable('b')) return 'done';
         }}
         f() + '|' + log.join()"
    );
    assert_eq!(
        evaluate(&source),
        Value::String("done|close b,close a".into())
    );
}

#[test]
fn a_failing_close_replaces_the_return_and_a_finalizer_return_still_closes() {
    let source = format!(
        "{ITERABLE}
         function f() {{ for (var x of iterable('t', function () {{ throw 'boom'; }})) return 1; }}
         function g() {{ for (var x of iterable('u')) {{ try {{ return 1; }} finally {{ return 2; }} }} }}
         var caught; try {{ f(); }} catch (e) {{ caught = e; }}
         [caught, g(), log.join()].join('|')"
    );
    assert_eq!(
        evaluate(&source),
        Value::String("boom|2|close t,close u".into())
    );
}

#[test]
fn a_generator_return_inside_a_loop_closes_the_iterator_after_finalizers() {
    let source = format!(
        "{ITERABLE}
         function* g() {{ for (var x of iterable('gen')) {{ try {{ yield 1; }} finally {{ log.push('finally'); }} }} }}
         var it = g(); it.next(); var r = it.return(5);
         r.value + '|' + r.done + '|' + log.join()"
    );
    assert_eq!(
        evaluate(&source),
        Value::String("5|true|finally,close gen".into())
    );
}
