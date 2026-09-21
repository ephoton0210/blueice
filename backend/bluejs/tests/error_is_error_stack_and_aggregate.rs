// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Error.isError` and the `Error.prototype.stack` accessor pair.

use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    Vm::default()
        .execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n  -> {error:?}"))
}

/// Expects `true`; a script may return a string describing the first failure.
fn assert_true(source: &str) {
    match evaluate(source) {
        Value::Bool(true) => {}
        Value::String(reason) => panic!("{}", reason.to_utf8().unwrap()),
        other => panic!("{source}\n  -> {other:?}"),
    }
}

#[test]
fn is_error_checks_the_error_data_slot_only() {
    assert_true(
        r#"(function() {
          const d = Object.getOwnPropertyDescriptor(Error, "isError");
          if (!d || !d.writable || d.enumerable || !d.configurable) return "attributes";
          if (d.value.name !== "isError" || d.value.length !== 1) return "name/length";
          try { new d.value({}); return "constructor"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          class MyError extends TypeError {}
          const yes = [new Error(), new TypeError(), new RangeError("x"), new MyError(), Error("no new"),
                       new AggregateError([]), Object.create(new Error())];
          // Object.create(error) has no [[ErrorData]] of its own.
          if (!Error.isError(yes[0]) || !Error.isError(yes[1]) || !Error.isError(yes[2]) ||
              !Error.isError(yes[3]) || !Error.isError(yes[4]) || !Error.isError(yes[5])) return "an error was not recognised";
          if (Error.isError(yes[6])) return "an object inheriting from an error is not an error";
          const no = [undefined, null, 1, "s", true, Symbol(), 1n, {}, [], function() {}, Error, Error.prototype,
                      { __proto__: Error.prototype, message: "", stack: "" }, new Proxy(new Error(), {}), /x/,
                      Object.create(Error.prototype)];
          for (const value of no) if (Error.isError(value)) return "false positive: " + String(typeof value);
          if (TypeError.isError !== Error.isError) return "not inherited by NativeErrors";
          return true;
        })()"#,
    );
}

#[test]
fn stack_is_an_accessor_on_error_prototype_only() {
    assert_true(
        r#"(function() {
          const d = Object.getOwnPropertyDescriptor(Error.prototype, "stack");
          if (!d || typeof d.get !== "function" || typeof d.set !== "function") return "not an accessor pair";
          if (d.enumerable || !d.configurable) return "attributes";
          if (d.get.name !== "get stack" || d.get.length !== 0) return "getter name/length";
          if (d.set.name !== "set stack" || d.set.length !== 1) return "setter name/length";
          for (const C of [Error, TypeError, RangeError, AggregateError]) {
            const e = C === AggregateError ? new C([]) : new C("m");
            if (Object.prototype.hasOwnProperty.call(e, "stack")) return C.name + " instance has an own stack";
            if (typeof e.stack !== "string") return C.name + " stack is not a string";
          }
          if (Object.getOwnPropertyDescriptor(TypeError.prototype, "stack") !== undefined) return "NativeError.prototype.stack";
          return true;
        })()"#,
    );
}

#[test]
fn the_stack_getter_returns_undefined_without_error_data_and_throws_for_non_objects() {
    assert_true(
        r#"(function() {
          const get = Object.getOwnPropertyDescriptor(Error.prototype, "stack").get;
          for (const v of [{}, Object.create(null), [], function() {}, /x/, new Map(), Error.prototype, Object.create(new Error()),
                           new Proxy(new Error(), {})]) {
            if (get.call(v) !== undefined) return "expected undefined";
          }
          const fake = Object.create(Error.prototype);
          Object.defineProperty(fake, "stack", { value: "imposter", writable: true, enumerable: true, configurable: true });
          if (get.call(fake) !== undefined) return "own data property is not consulted";
          for (const v of [undefined, null, true, 1, "", Symbol(), 1n]) {
            try { get.call(v); return "primitive receiver accepted"; }
            catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          }
          if (typeof get.call(new Error("boom")) !== "string") return "not a string";
          return true;
        })()"#,
    );
}

#[test]
fn the_stack_setter_ignores_the_prototype_property() {
    assert_true(
        r#"(function() {
          const set = Object.getOwnPropertyDescriptor(Error.prototype, "stack").set;
          const err = new Error("m");
          if (set.call(err, "sentinel") !== undefined) return "setter result";
          const own = Object.getOwnPropertyDescriptor(err, "stack");
          if (own.value !== "sentinel" || !own.writable || !own.enumerable || !own.configurable) return "own data property";
          if (err.stack !== "sentinel") return "read back";
          const plain = {};
          set.call(plain, "plain");
          if (plain.stack !== "plain") return "plain object";
          // A non-String value is rejected, without coercion, before the receiver is touched.
          for (const bad of [undefined, null, 1, {}, { toString() { return "x"; } }, new String("boxed"), Symbol(), 1n]) {
            try { set.call(new Error("m"), bad); return "non-string accepted"; }
            catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          }
          try { set.call(new Error("m")); return "no argument accepted"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          for (const bad of [undefined, null, 1, "s"]) {
            try { set.call(bad, "x"); return "primitive receiver accepted"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          }
          // Setting on %Error.prototype% itself always throws, including through assignment.
          try { set.call(Error.prototype, ""); return "Error.prototype accepted"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          try { Error.prototype.stack = ""; return "assignment accepted"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          // Assignment to an instance goes through the setter and creates the own property.
          const viaAssign = new TypeError("t");
          viaAssign.stack = "assigned";
          if (viaAssign.stack !== "assigned" || !Object.prototype.hasOwnProperty.call(viaAssign, "stack")) return "assignment";
          // Before an own property exists the inherited setter runs, and its TypeError propagates.
          try { new TypeError("t").stack = 1; return "numeric assignment accepted"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          // An existing own property goes through [[Set]] with Throw = true.
          const existing = new Error("m");
          Object.defineProperty(existing, "stack", { value: "a", writable: false, enumerable: false, configurable: true });
          try { set.call(existing, "b"); return "non-writable accepted"; } catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          let observed;
          const accessor = new Error("m");
          Object.defineProperty(accessor, "stack", { get() { return observed; }, set(v) { observed = v; }, configurable: true });
          set.call(accessor, "via own setter");
          if (observed !== "via own setter") return "own setter";
          // Proxies observe [[GetOwnProperty]] and [[DefineOwnProperty]].
          const log = [];
          const proxy = new Proxy({}, {
            getOwnPropertyDescriptor(t, k) { log.push("gopd " + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
            defineProperty(t, k, desc) { log.push("define " + String(k) + " " + desc.value); return Reflect.defineProperty(t, k, desc); },
          });
          set.call(proxy, "via proxy");
          if (log.join() !== "gopd stack,define stack via proxy") return "proxy traps: " + log.join();
          try { set.call(new Proxy({}, { defineProperty() { return false; } }), "v"); return "rejecting proxy accepted"; }
          catch (e) { if (!(e instanceof TypeError)) return "wrong error"; }
          return true;
        })()"#,
    );
}
