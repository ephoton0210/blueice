// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private names as destructuring targets and after `?.`, private accessor
//! reads, and the class-body grammar corners the Test262 class scope covers.

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let result = Vm::default().execute(&code);
    assert_eq!(result, Ok(Value::Bool(true)), "{source}");
}

#[test]
fn a_private_field_is_a_valid_destructuring_and_for_target() {
    assert_true(
        "class C {
             #a; #b; #c;
             static run() {
                 const o = new C();
                 [o.#a] = [1];
                 ({ x: o.#b } = { x: 2 });
                 for (o.#c of [3]) {}
                 [...o.#a] = [4, 5];
                 return o.#a.join() + '/' + o.#b + '/' + o.#c;
             }
         }
         C.run() === '4,5/2/3'",
    );
    // PrivateFieldSet on an object without the field is a TypeError.
    assert_true(
        "class C { #f; m() { for (this.#f of [1]) ; } n() { [...this.#f] = []; } p() { [this.#f] = [1]; } }
         const errors = [];
         for (const name of ['m', 'n', 'p']) {
             try { C.prototype[name].call({}); } catch (e) { errors.push(e instanceof TypeError); }
         }
         errors.join() === 'true,true,true'",
    );
    // The target is resolved before the value is read: a getter that adds the
    // private field to `this` makes the later PrivateFieldSet succeed.
    assert_true(
        "class Base { constructor(o) { return o; } }
         class C extends Base {
             #field;
             m() {
                 const init = () => new C(this);
                 const object = { get a() { init(); return 'pass'; } };
                 ({ a: this.#field } = object);
                 return this.#field;
             }
         }
         C.prototype.m.call({}) === 'pass'",
    );
}

#[test]
fn a_private_name_is_valid_after_an_optional_chain_link() {
    assert_true(
        "class C {
             #f = 'field'; #m() { return 'method'; }
             static field(o) { return o?.#f; }
             static deep(o) { return o?.c.#f; }
             static method(o) { return o?.#m(); }
             static parenthesized(o) { return (o?.#m)(); }
         }
         const c = new C();
         C.field(c) === 'field' && C.field(null) === undefined && C.field(undefined) === undefined
             && C.deep({ c }) === 'field' && C.deep(null) === undefined
             && C.method(c) === 'method' && C.method(null) === undefined
             && C.parenthesized(c) === 'method'",
    );
    assert_true(
        "class C { #f; static access(o) { return o?.#f; } static deep(o) { return o?.c.#f; } }
         let a, b;
         try { C.access({}); } catch (e) { a = e; }
         try { C.deep({ c: {} }); } catch (e) { b = e; }
         a instanceof TypeError && b instanceof TypeError",
    );
}

#[test]
fn reading_a_private_accessor_without_a_getter_is_a_type_error() {
    assert_true(
        "class C { set #f(v) {} get() { return this.#f; } static set #s(v) {} static read() { return C.#s; } }
         let a, b;
         try { new C().get(); } catch (e) { a = e; }
         try { C.read(); } catch (e) { b = e; }
         a instanceof TypeError && b instanceof TypeError",
    );
    // An inner class's private setter shadows the outer class's getter.
    assert_true(
        "class Outer {
             get #m() { return 'outer'; }
             method() { return this.#m; }
             Inner = class {
                 method(o) { return o.#m; }
                 set #m(v) {}
             };
         }
         const o = new Outer(), inner = new o.Inner();
         let a, b;
         try { inner.method(inner); } catch (e) { a = e; }
         try { inner.method(o); } catch (e) { b = e; }
         o.method() === 'outer' && a instanceof TypeError && b instanceof TypeError",
    );
}

#[test]
fn the_in_operator_is_available_in_class_bodies_inside_a_for_head() {
    assert_true(
        "var empty = Object.create(null), C, value;
         for (C = class { get ['x' in empty]() { return 'via get'; } }; ; ) {
             value = C.prototype.false;
             break;
         }
         value === 'via get'",
    );
}

#[test]
fn get_and_set_are_names_unless_a_property_name_follows() {
    assert_true(
        "class A { get
                   *a() {} }
         class B { static get
                   *a() {} }
         A.prototype.hasOwnProperty('a') && new A().hasOwnProperty('get')
             && B.prototype.hasOwnProperty('a') && B.hasOwnProperty('get')",
    );
    assert_true(
        "class C { get = 1; set; static get; get() { return 'method'; } }
         new C().get === 1 && new C().hasOwnProperty('set') && C.hasOwnProperty('get')",
    );
    assert_true(
        "class C { get x() { return 1; } set x(v) {} get 'y'() { return 2; } get 3() { return 3; }
                   get [`z`]() { return 4; } }
         const p = new C();
         p.x === 1 && p.y === 2 && p[3] === 3 && p.z === 4",
    );
}

#[test]
fn an_auto_accessor_is_a_getter_setter_pair_over_hidden_storage() {
    assert_true(
        "let name = 'x3'; const symbol = Symbol();
         class C {
             accessor x0; accessor x1 = 1; accessor 'x2' = 2; accessor [name] = 3;
             accessor [symbol] = 4; accessor 1 = 5;
             static accessor s = 6;
         }
         const c = new C();
         const own = Object.getOwnPropertyDescriptor(C.prototype, 'x1');
         c.x0 === undefined && c.x1 === 1 && c.x2 === 2 && c.x3 === 3 && c[symbol] === 4
             && c[1] === 5 && C.s === 6
             && typeof own.get === 'function' && typeof own.set === 'function'
             && own.get.name === 'get x1' && own.set.name === 'set x1'
             && !own.enumerable && own.configurable && !c.hasOwnProperty('x1')
             && (c.x1 = 43, c.x1 === 43) && (C.s = 7, C.s === 7)",
    );
    // Each instance has its own storage, initialized in field order.
    assert_true(
        "let n = 0;
         class C { accessor a = ++n; b = ++n; accessor c = ++n; }
         const one = new C(), two = new C();
         one.a === 1 && one.b === 2 && one.c === 3 && two.a === 4 && two.c === 6 && one.a === 1",
    );
    // A later accessor with the same name replaces the earlier one.
    assert_true(
        "class C { accessor x = 0; accessor x = 1; }
         new C().x === 1",
    );
    // A computed name is evaluated once for the whole pair.
    assert_true(
        "let calls = 0;
         class C { accessor [(calls++, 'k')] = 1; }
         const c = new C(); c.k = 2;
         calls === 1 && c.k === 2",
    );
}

#[test]
fn a_private_auto_accessor_is_a_private_getter_and_setter() {
    assert_true(
        "class C {
             accessor #x = 5; static accessor #s = 6;
             read() { return this.#x; } write(v) { this.#x = v; }
             static readStatic() { return C.#s; }
         }
         const c = new C();
         const before = c.read(); c.write(42);
         before === 5 && c.read() === 42 && C.readStatic() === 6",
    );
    for source in [
        "class C { accessor #x = 5; accessor #x = 42; }",
        "class C { accessor #x = 5; #x = 42; }",
        "class C { accessor #x = 5; get #x() {} }",
        "class C { accessor #x = 5; set #x(value) {} }",
    ] {
        let program = parse(source);
        let rejected = match program {
            Err(_) => true,
            Ok(program) => compile(&program).is_err(),
        };
        assert!(rejected, "{source}");
    }
}

#[test]
fn accessor_is_a_plain_name_unless_a_class_element_name_follows_on_the_same_line() {
    assert_true(
        "class C { accessor; }
         class D { accessor = 42; }
         class E { accessor
                   a = 42; }
         class F { accessor() { return 'method'; } }
         new C().hasOwnProperty('accessor') && new D().accessor === 42
             && new E().a === 42 && new E().hasOwnProperty('accessor') && new F().accessor() === 'method'",
    );
}
