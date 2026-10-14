// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `++`/`--` on a private member run PrivateGet then PrivateSet on the
//! private element, never an ordinary property read and write of the name.
use blueice_bluejs::{compile, parse, Value, Vm};

fn evaluate(source: &str) -> Value {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default()
        .execute(&compile(&program).unwrap())
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

fn assert_true(source: &str) {
    assert_eq!(evaluate(source), Value::Bool(true), "{source}");
}

#[test]
fn postfix_and_prefix_update_a_private_field() {
    assert_true(
        "class C { #a = 1;
           post() { return [this.#a++, this.#a]; }
           pre() { return [++this.#a, this.#a]; }
           postDec() { return [this.#a--, this.#a]; }
           preDec() { return [--this.#a, this.#a]; } }
         var c = new C();
         var r = [c.post(), c.pre(), c.postDec(), c.preDec()].join('|');
         r === '1,2|3,3|3,2|1,1'",
    );
}

#[test]
fn an_update_reads_the_private_element_once_and_writes_it_once() {
    assert_true(
        "var log = [];
         class C { #v = 5;
           get #a() { log.push('get'); return this.#v; }
           set #a(x) { log.push('set:' + x); this.#v = x; }
           run() { return this.#a++; } }
         var r = new C().run();
         r === 5 && log.join() === 'get,set:6'",
    );
}

#[test]
fn an_update_converts_the_old_value_with_tonumeric_and_keeps_bigint() {
    assert_true(
        "class C { #s = '41'; #b = 9007199254740993n;
           s() { return [this.#s++, this.#s]; }
           b() { return [this.#b++, this.#b]; } }
         var c = new C();
         var s = c.s(), b = c.b();
         s[0] === 41 && s[1] === 42 && b[0] === 9007199254740993n && b[1] === 9007199254740994n",
    );
}

#[test]
fn an_update_of_a_foreign_receiver_or_a_private_method_throws_a_type_error() {
    assert_true(
        "class C { #a = 1; #m() {}
           static inc(o) { o.#a++; }
           static incMethod(o) { o.#m++; } }
         var brand = 0, method = 0;
         try { C.inc({}); } catch (e) { brand = e instanceof TypeError ? 1 : 2; }
         try { C.incMethod(new C()); } catch (e) { method = e instanceof TypeError ? 1 : 2; }
         brand === 1 && method === 1",
    );
}

#[test]
fn a_private_field_of_a_frozen_object_is_still_updatable() {
    assert_true(
        "class Base { constructor(o) { return o; } }
         class A extends Base { #a = 1;
           static gs(o) { return o.#a; }
           static inca(o) { o.#a++; } }
         var obj = {};
         new A(obj);
         Object.freeze(obj);
         A.inca(obj);
         A.gs(obj) === 2 && Object.isFrozen(obj)",
    );
}
