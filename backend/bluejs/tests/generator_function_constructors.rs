// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `%GeneratorFunction%` and `%AsyncGeneratorFunction%` (ECMA-262 27.3, 27.4):
//! the intrinsic constructors, the prototype objects that link them to
//! `%GeneratorPrototype%` / `%AsyncGeneratorPrototype%`, and the async
//! generator methods' handling of a bad receiver or a value whose
//! PromiseResolve throws.

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
fn generator_function_builds_generator_functions() {
    check(
        r#"
        var GeneratorFunction = Object.getPrototypeOf(function*() {}).constructor;
        check(GeneratorFunction.name === "GeneratorFunction" && GeneratorFunction.length === 1, "constructor shape");
        var g = GeneratorFunction("x", "y", "yield x + y; return 'end';");
        check(typeof g === "function" && g.name === "anonymous" && g.length === 2, "function shape");
        check(Object.getPrototypeOf(g) === GeneratorFunction.prototype, "prototype of the function");
        check(g instanceof GeneratorFunction, "instanceof");
        // Calling the constructor with new behaves the same, and a parameter-list argument may hold several names.
        var h = new GeneratorFunction("a, b", "yield a; yield b;");
        check(h.length === 2, "comma separated parameters");
        var it = g(2, 3);
        var first = it.next();
        var second = it.next();
        var third = it.next();
        check(first.value === 5 && !first.done, "first step " + first.value);
        check(second.value === "end" && second.done, "second step " + second.value + " " + second.done);
        check(third.value === undefined && third.done, "third step");
        // The function owns the prototype its instances inherit from: no `constructor`, and %GeneratorPrototype% above it.
        var d = Object.getOwnPropertyDescriptor(g, "prototype");
        check(d && d.writable && !d.enumerable && !d.configurable, "prototype attributes");
        check(Object.getPrototypeOf(g.prototype) === GeneratorFunction.prototype.prototype, "generator prototype chain");
        check(!Object.prototype.hasOwnProperty.call(g.prototype, "constructor"), "no constructor link");
        check(Object.getPrototypeOf(it) === g.prototype, "instance prototype");
        try { new g(); check(false, "generator function constructed"); } catch (e) { check(e instanceof TypeError, "construct error"); }
        // A script-level variable must survive the generator's own frame (its `arguments` binding shares slot numbers).
        var survivor = { kept: true };
        var lazy = GeneratorFunction("yield 1; yield 2;")();
        lazy.next();
        lazy.next();
        check(survivor.kept === true && typeof lazy.next === "function", "script variables are untouched by generator frames");
        // Syntax rules of a generator: yield in a parameter is an early error.
        try { GeneratorFunction("a = yield", ""); check(false, "yield in parameters"); } catch (e) { check(e instanceof SyntaxError, "parameter yield error"); }
        // newTarget selects the prototype of the function.
        var proto = {};
        var C = function() {}.bind();
        C.prototype = proto;
        var made = Reflect.construct(GeneratorFunction, ["yield 1"], C);
        check(Object.getPrototypeOf(made) === proto, "newTarget prototype");
        finish();
        "#,
    );
}

#[test]
fn async_generator_functions_have_their_own_intrinsics() {
    check(
        r#"
        async function* source() {}
        var AsyncGeneratorFunctionPrototype = Object.getPrototypeOf(source);
        var AsyncFunctionPrototype = Object.getPrototypeOf(async function() {});
        check(AsyncGeneratorFunctionPrototype !== AsyncFunctionPrototype, "async generator functions do not share %AsyncFunction.prototype%");
        check(Object.getPrototypeOf(AsyncGeneratorFunctionPrototype) === Function.prototype, "inherits Function.prototype");
        var AsyncGeneratorFunction = AsyncGeneratorFunctionPrototype.constructor;
        check(AsyncGeneratorFunction.name === "AsyncGeneratorFunction" && AsyncGeneratorFunction.length === 1, "constructor shape");
        check(Object.getPrototypeOf(AsyncGeneratorFunction) === Function, "constructor inherits %Function%");
        check(AsyncGeneratorFunction.prototype === AsyncGeneratorFunctionPrototype, "constructor.prototype");
        var AsyncGeneratorPrototype = AsyncGeneratorFunctionPrototype.prototype;
        check(typeof AsyncGeneratorPrototype === "object" && typeof AsyncGeneratorPrototype.next === "function", "%AsyncGeneratorPrototype%");
        check(AsyncGeneratorPrototype.constructor === AsyncGeneratorFunctionPrototype, "constructor link back");
        var links = ["constructor", "prototype"].map(function(k) {
          var o = k === "constructor" ? AsyncGeneratorFunctionPrototype : AsyncGeneratorPrototype;
          var d = Object.getOwnPropertyDescriptor(o, k === "constructor" ? "constructor" : "constructor");
          return d;
        });
        var d = Object.getOwnPropertyDescriptor(AsyncGeneratorFunctionPrototype, "prototype");
        check(!d.writable && !d.enumerable && d.configurable, "prototype link attributes");
        d = Object.getOwnPropertyDescriptor(AsyncGeneratorPrototype, "constructor");
        check(!d.writable && !d.enumerable && d.configurable, "constructor link attributes");
        check(Object.prototype.toString.call(source) === "[object AsyncGeneratorFunction]", "toStringTag of a function");
        check(Object.getPrototypeOf(source.prototype) === AsyncGeneratorPrototype, "instance prototype chain");
        // Methods in classes and object literals get the same prototype.
        var o = { async *m() {} };
        check(Object.getPrototypeOf(o.m) === AsyncGeneratorFunctionPrototype, "method prototype");
        // The constructor compiles `async function*` from its arguments.
        var made = AsyncGeneratorFunction("a", "yield a; yield await Promise.resolve(a + 1);");
        check(made.name === "anonymous" && made.length === 1, "dynamic function shape");
        check(Object.getPrototypeOf(made) === AsyncGeneratorFunctionPrototype, "dynamic prototype");
        check(Object.getPrototypeOf(made.prototype) === AsyncGeneratorPrototype, "dynamic instance prototype");
        try { AsyncGeneratorFunction("await", ""); check(false, "await as a parameter name"); } catch (e) { check(e instanceof SyntaxError, "await parameter error"); }
        var seen = [];
        var it = made(41);
        it.next().then(function(r) {
          seen.push(r.value);
          return it.next();
        }).then(function(r) {
          seen.push(r.value);
          return it.next();
        }).then(function(r) {
          check(seen.join() === "41,42" && r.done, "iteration: " + seen.join());
          finish();
        });
        "#,
    );
}

#[test]
fn async_generator_methods_reject_instead_of_throwing_for_a_bad_receiver() {
    check(
        r#"
        async function* g() {}
        var proto = Object.getPrototypeOf(g).prototype;
        function* sync() {}
        var pending = [];
        for (var name of ["next", "return", "throw"]) {
          for (var bad of [undefined, null, 1, "s", {}, [], function() {}, g, g.prototype, sync(), proto]) {
            var promise;
            try { promise = proto[name].call(bad, 1); } catch (e) { check(false, name + " threw synchronously"); continue; }
            check(promise instanceof Promise, name + " did not return a promise");
            pending.push(promise.then(function() { check(false, "fulfilled"); }, function(e) { check(e instanceof TypeError, "rejected with a TypeError"); }));
          }
        }
        Promise.all(pending).then(finish);
        "#,
    );
}

#[test]
fn return_awaits_its_operand_even_for_a_completed_generator() {
    check(
        r#"
        var hostile = Promise.resolve(42);
        Object.defineProperty(hostile, "constructor", { get() { throw new EvalError("broken promise"); } });
        var order = [];
        async function* g() { yield 1; }
        var done = g();
        done.next().then(() => done.next()).then(function() {
          // The generator is completed now.
          var value = done.return(Promise.resolve("awaited"));
          order.push("returned");
          return value.then(function(r) {
            check(r.value === "awaited" && r.done === true, "completed generator awaits the operand: " + String(r.value));
            return done.return(hostile);
          });
        }).then(function() { check(false, "hostile constructor fulfilled"); }, function(e) {
          check(e instanceof EvalError, "hostile constructor rejects the request");
        }).then(function() {
          // A generator that never started rejects the same way, without running its body.
          var body = 0;
          var fresh = (async function*() { body++; })();
          return fresh.return(hostile).then(() => check(false, "fresh fulfilled"), function(e) {
            check(e instanceof EvalError && body === 0, "fresh generator: " + body);
          });
        }).then(finish);
        "#,
    );
}

#[test]
fn return_at_a_yield_throws_a_broken_operand_into_the_generator() {
    check(
        r#"
        var caught;
        var g = async function*() {
          try { yield; return "never returned"; }
          catch (err) { caught = err; return 1; }
        };
        var hostile = Promise.resolve(42);
        Object.defineProperty(hostile, "constructor", { get() { throw new EvalError("broken promise"); } });
        var it = g();
        it.next().then(function() { return it.return(hostile); }).then(function(r) {
          check(caught instanceof EvalError && caught.message === "broken promise", "the body did not catch the error");
          check(r.value === 1 && r.done === true, "result " + r.value + " " + r.done);
          // An ordinary operand still finishes with its awaited value; a finally block still runs.
          var ran = [];
          var h = async function*() { try { yield 1; } finally { ran.push("finally"); } };
          var it2 = h();
          return it2.next().then(function() { return it2.return(Promise.resolve("plain")); }).then(function(r2) {
            check(r2.value === "plain" && r2.done === true && ran.join() === "finally", "plain return " + r2.value + " " + ran.join());
          });
        }).then(finish);
        "#,
    );
}
