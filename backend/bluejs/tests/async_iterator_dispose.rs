// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `%AsyncIteratorPrototype% [ @@asyncDispose ] ( )`: calls the iterator's
//! `return` (without arguments, as Test262 requires) and reports the outcome as a promise that
//! fulfils with `undefined`; every failure becomes a rejection.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn run(script: &str, gc_stress: bool) {
    let mut config = VmConfig::default();
    if gc_stress {
        config.heap.nursery_capacity = 1;
    }
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_done().unwrap();
    let source = format!(
        "var log = [];\
         function check(condition, message) {{ if (!condition) log.push(message); }}\
         function finish() {{ if (log.length === 0) $DONE(); else $DONE(log.join('; ')); }}\
         {script}"
    );
    vm.execute_script(&compile(&parse(&source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{script}\n=> {error:?}"));
    vm.run_promise_jobs().unwrap();
    match vm
        .take_test262_done()
        .expect("the script never reported through $DONE")
    {
        Ok(()) => {}
        Err(Value::String(problems)) => panic!(
            "{script}\n(gc_stress = {gc_stress}) => {}",
            problems.to_utf8().unwrap()
        ),
        Err(other) => panic!("{script}\n(gc_stress = {gc_stress}) => {other:?}"),
    }
}

fn check(script: &str) {
    run(script, false);
    run(script, true);
}

#[test]
fn the_method_has_its_specified_shape() {
    check(
        r#"
        async function* generator() {}
        var AsyncIteratorPrototype = Object.getPrototypeOf(Object.getPrototypeOf(generator.prototype));
        var d = Object.getOwnPropertyDescriptor(AsyncIteratorPrototype, Symbol.asyncDispose);
        check(d && typeof d.value === "function", "missing");
        check(d.writable && !d.enumerable && d.configurable, "attributes");
        check(d.value.name === "[Symbol.asyncDispose]" && d.value.length === 0, "name/length: " + d.value.name + " " + d.value.length);
        try { new d.value(); check(false, "constructor"); } catch (e) { check(e instanceof TypeError, "constructor error"); }
        check(AsyncIteratorPrototype[Symbol.asyncDispose]() instanceof Promise, "does not return a promise");
        finish();
        "#,
    );
}

#[test]
fn it_calls_return_without_arguments_and_fulfils_with_undefined() {
    check(
        r#"
        async function* generator() {}
        var AsyncIteratorPrototype = Object.getPrototypeOf(Object.getPrototypeOf(generator.prototype));
        var seen = [];
        var iter = Object.create(AsyncIteratorPrototype);
        iter.return = async function() { seen.push(arguments.length, this === iter); return { done: true, value: "ignored" }; };
        var noReturn = Object.create(AsyncIteratorPrototype);
        var nullReturn = { return: null };
        Promise.all([
          iter[Symbol.asyncDispose](),
          noReturn[Symbol.asyncDispose](),
          AsyncIteratorPrototype[Symbol.asyncDispose].call(nullReturn),
          AsyncIteratorPrototype[Symbol.asyncDispose].call({ return() { return 5; } }),
          AsyncIteratorPrototype[Symbol.asyncDispose].call({ return() { return { then(f) { f("thenable"); } }; } }),
        ]).then(function(values) {
          check(values.every(v => v === undefined), "values: " + values.join());
          check(seen.join() === "0,true", "return call: " + seen.join());
          finish();
        });
        "#,
    );
}

#[test]
fn every_failure_becomes_a_rejection() {
    check(
        r#"
        async function* generator() {}
        var AsyncIteratorPrototype = Object.getPrototypeOf(Object.getPrototypeOf(generator.prototype));
        var dispose = AsyncIteratorPrototype[Symbol.asyncDispose];
        function CatchError() {}
        var cases = [
          [{ get return() { throw new CatchError(); } }, CatchError, "getter throws"],
          [{ return() { throw new CatchError(); } }, CatchError, "return throws"],
          [{ return() { return Promise.reject(new CatchError()); } }, CatchError, "return rejects"],
          [{ return: 1 }, TypeError, "return is not callable"],
          [undefined, TypeError, "undefined receiver"],
          [null, TypeError, "null receiver"],
          [{ return() { var p = Promise.resolve(1); Object.defineProperty(p, "constructor", { get() { throw new CatchError(); } }); return p; } }, CatchError, "hostile promise"],
        ];
        var pending = cases.map(function(c) {
          var promise;
          try { promise = dispose.call(c[0]); } catch (e) { check(false, c[2] + " threw synchronously"); return Promise.resolve(); }
          return promise.then(() => check(false, c[2] + " fulfilled"), e => check(e instanceof c[1], c[2] + " rejected with the wrong error"));
        });
        Promise.all(pending).then(finish);
        "#,
    );
}
