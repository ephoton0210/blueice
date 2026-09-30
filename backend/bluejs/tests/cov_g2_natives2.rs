// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! More rarely taken completions of the built-in natives, property access and
//! class machinery: revoked proxies, throwing accessors, foreign-realm
//! receivers, private methods next to uninitialized fields and constrained
//! string lengths.
mod cov_g2_support;

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};
use cov_g2_support::{expect_script_true, expect_true};

const PRELUDE: &str = "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
    function isType(f) { return thrown(f) instanceof TypeError; }
    var boom = { valueOf() { throw 'boom'; }, toString() { throw 'boom'; } };
    var revoked = Proxy.revocable({}, {}); revoked.revoke(); revoked = revoked.proxy;";

fn check(body: &str) {
    expect_true(&format!("{PRELUDE}\n{body}"));
}

#[test]
fn number_predicates_and_typed_array_intrinsic() {
    check(
        "var TAC = Object.getPrototypeOf(Int8Array);
         isType(() => TAC()) && isType(() => new TAC()) &&
           Number.isFinite(1) && !Number.isFinite(Infinity) && !Number.isFinite('1') &&
           Number.isInteger(2) && !Number.isInteger(1.5) && !Number.isInteger(Infinity) && !Number.isInteger('2') &&
           Number.isSafeInteger(3) && !Number.isSafeInteger(2 ** 53) && !Number.isSafeInteger(1.5) && !Number.isSafeInteger('3') &&
           Number.isNaN(NaN) && !Number.isNaN('x')",
    );
}

#[test]
fn revoked_proxies_fail_property_and_array_checks() {
    check(
        "isType(() => Array.isArray(revoked)) && isType(() => revoked.x) && isType(() => { revoked.x = 1; }) &&
           isType(() => Object.setPrototypeOf([], revoked).push(1)) &&
           isType(() => Object.prototype.toString.call(revoked)) &&
           thrown(() => new Proxy({}, { getOwnPropertyDescriptor() { throw 'gopd'; } }).__lookupGetter__('x')) === 'gopd' &&
           thrown(() => new Proxy({}, { getPrototypeOf() { throw 'gpo'; } }).__lookupGetter__('x')) === 'gpo'",
    );
}

#[test]
fn accessors_conversions_and_receivers_of_array_natives() {
    check(
        "thrown(() => Array.prototype.values.call({ length: boom }).next()) === 'boom' &&
           thrown(() => Object.prototype.toLocaleString.call({ get toString() { throw 'ts'; } })) === 'ts' &&
           isType(() => Array.prototype.toString.call(null)) &&
           thrown(() => Array.prototype.toString.call({ get join() { throw 'join'; } })) === 'join' &&
           (function () { var a = [1, 2]; return [...a.entries()].length === 2 && [...a.keys()].join() === '0,1'; })()",
    );
}

#[test]
fn super_keys_and_setters_report_conversion_errors() {
    check(
        "class A { set s(v) { throw 'setter'; } }
         class B extends A {
           static get(k) { return super[k]; }
           static set(k) { super[k] = 1; }
           m(k) { return super[k]; }
         }
         thrown(() => B.get(boom)) === 'boom' && thrown(() => B.set(boom)) === 'boom' && thrown(() => new B().m(boom)) === 'boom'",
    );
}

#[test]
fn private_methods_expose_fields_that_are_not_yet_initialized() {
    check(
        "class Read { a = this.#b; #b = 1; #m() {} }
         class Write { a = (this.#b = 2); #b = 1; #m() {} }
         class Setter { set #s(v) { throw 'set'; } static run(o) { o.#s = 1; } }
         isType(() => new Read()) && isType(() => new Write()) &&
           thrown(() => Setter.run(new Setter())) === 'set'",
    );
}

#[test]
fn computed_class_names_respect_existing_names() {
    check(
        "var o = { ['k1']: class {}, ['k2']: class { static name() {} }, ['k3']: function () {}, [Symbol('sym')]: class {} };
         var syms = Object.getOwnPropertySymbols(o);
         o.k1.name === 'k1' && typeof o.k2.name === 'function' && o.k3.name === 'k3' && o[syms[0]].name === '[sym]'",
    );
}

#[test]
fn global_object_writes_go_through_the_global_binding() {
    expect_script_true(
        "var g = 1; Object.defineProperty(globalThis, 'g', { writable: false });
         globalThis.g = 2;
         var strictFailed = (function () { 'use strict'; try { globalThis.g = 3; return false; } catch (e) { return e instanceof TypeError; } })();
         var writable = 1; globalThis.writable = 5;
         strictFailed && g === 1 && writable === 5",
    );
}

#[test]
fn foreign_realm_typed_arrays_dispatch_through_the_bridge() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "function getter(o, n) { return Object.getOwnPropertyDescriptor(o, n).get; }
        var other = $262.createRealm().global;
        var foreign = new other.Int8Array(4);
        var TA = Object.getPrototypeOf(Int8Array.prototype);
        TA.fill.call(foreign, 7);
        TA.set.call(foreign, [1, 2]);
        var joined = TA.join.call(foreign, ',');
        var sub = TA.subarray.call(foreign, 1, 3);
        joined === '1,2,7,7' && sub.length === 2 && getter(TA, 'length').call(foreign) === 4 &&
          getter(TA, 'byteLength').call(foreign) === 4";
    let result = vm.execute(&compile(&parse(source).unwrap()).unwrap());
    assert_eq!(result, Ok(Value::Bool(true)));
}

#[test]
fn symbol_conversion_is_bounded_by_the_string_limit() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 100,
        ..VmConfig::default()
    })
    .unwrap();
    let source = format!("String(Symbol('{}'))", "x".repeat(45));
    assert_eq!(
        vm.execute(&compile(&parse(&source).unwrap()).unwrap()),
        Err(RuntimeError::StringLimit { limit: 100 })
    );
}
