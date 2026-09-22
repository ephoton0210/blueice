// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class element evaluation order and the [[Fields]] initializer: computed
//! keys are converted once when the class is defined, fields are defined by a
//! method-like function run at construction (or when `super()` returns), and
//! static elements run only after every element has been defined.

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let result = Vm::default().execute(&code);
    assert_eq!(result, Ok(Value::Bool(true)), "{source}");
}

#[test]
fn computed_field_keys_are_evaluated_once_in_order_before_static_fields_run() {
    assert_true(
        "let i = 0;
         var C = class {
             [i++] = i++;
             static [i++] = i++;
             [i++] = i++;
         };
         const c = new C();
         // Keys 0, 1, 2 are taken first, then the static initializer (3) runs
         // at definition, then each instance runs its initializers (4, 5).
         c[0] === 4 && c[2] === 5 && C[1] === 3 && i === 6
             && !c.hasOwnProperty('1') && !C.hasOwnProperty('0')",
    );
    assert_true(
        "let x = 1;
         var C = class { [x++] = x++; [x++] = x++; };
         const c1 = new C(), c2 = new C();
         c1[1] === 3 && c1[2] === 4 && c2[1] === 5 && c2[2] === 6",
    );
}

#[test]
fn an_abrupt_computed_key_stops_the_class_definition() {
    assert_true(
        "let ran = false, err;
         try {
             class C { [(() => { throw new RangeError('boom'); })()] = 1; [ran = true]; }
         } catch (e) { err = e; }
         err instanceof RangeError && ran === false",
    );
    assert_true(
        "let err;
         const key = { toString() { throw new EvalError('tostring'); } };
         try { class C { [key]; } } catch (e) { err = e; }
         err instanceof EvalError",
    );
    assert_true(
        "let err;
         try { class C { [noSuchVariable] = 1; } } catch (e) { err = e; }
         err instanceof ReferenceError",
    );
}

#[test]
fn the_class_name_is_unavailable_to_computed_keys_but_available_to_initializers() {
    assert_true(
        "let err;
         try { class C { [C.name]() {} } } catch (e) { err = e; }
         err instanceof ReferenceError",
    );
    assert_true(
        "class C { static self = C; static viaBlock; static { C.viaBlock = C; } m() { return C; } }
         C.self === C && C.viaBlock === C && new C().m() === C",
    );
}

#[test]
fn a_class_declaration_has_an_immutable_inner_binding_and_a_mutable_outer_one() {
    assert_true(
        "class C { method() { return C; } }
         const cls = C;
         C = null;
         C === null && cls.prototype.method() === cls",
    );
    assert_true(
        "class C { constructor() { C = 42; } }
         let err;
         try { new C(); } catch (e) { err = e; }
         err instanceof TypeError",
    );
    assert_true(
        "var setHeritage;
         class C extends (setHeritage = function() { C = null; }, Object) {}
         let err;
         try { setHeritage(); } catch (e) { err = e; }
         err instanceof TypeError && typeof C === 'function'",
    );
    // The outer binding is not initialized until the class is fully defined,
    // but the inner one is initialized before static initializers run.
    assert_true(
        "let inside;
         { try { class D { static probe = (inside = typeof D); } } catch (e) {} }
         inside === 'function'",
    );
}

#[test]
fn field_initializers_do_not_see_constructor_parameters_or_new_target() {
    assert_true(
        "var y = 'outer';
         class C { x = y; constructor(y) { this.param = y; } }
         const c = new C('param');
         c.x === 'outer' && c.param === 'param'",
    );
    assert_true(
        "class C { seen = () => new.target; direct = eval('new.target'); }
         const c = new C();
         c.seen() === undefined && c.direct === undefined",
    );
    assert_true(
        "class C { x = eval('arguments'); }
         let err;
         try { new C(); } catch (e) { err = e; }
         err instanceof SyntaxError",
    );
    assert_true(
        "class C { x = () => eval('arguments'); }
         let err;
         try { new C().x(); } catch (e) { err = e; }
         err instanceof SyntaxError",
    );
}

#[test]
fn fields_are_initialized_when_construction_or_super_returns() {
    assert_true(
        "const log = [];
         class B { constructor() { log.push('base ctor'); } }
         class C extends B {
             field = log.push('field');
             constructor() {
                 log.push('before super');
                 super();
                 log.push('after super');
             }
         }
         new C();
         log.join() === 'before super,base ctor,field,after super'",
    );
    assert_true(
        "let base = 0, field = 0;
         class B { constructor() { base++; } }
         const C = class extends B {
             f = ++field;
             constructor() {
                 super();
                 let err;
                 try { super(); } catch (e) { err = e; }
                 this.err = err;
             }
         };
         const c = new C();
         base === 2 && field === 1 && c.err instanceof ReferenceError",
    );
    assert_true(
        "let order = [];
         class C { a = order.push('field'); constructor(p = order.push('param')) { order.push('body'); } }
         new C();
         order.join() === 'field,param,body'",
    );
    assert_true(
        "let thisInField, fromArrow, probe;
         class B { constructor() { } }
         class C extends B {
             field = (thisInField = this, fromArrow = probe());
             constructor() { probe = () => this; super(); }
         }
         const c = new C();
         thisInField === c && fromArrow === c",
    );
}

#[test]
fn fields_are_defined_not_assigned_and_named_after_their_keys() {
    assert_true(
        "class C {
             a = function() {}; b = () => 1; c = class {}; ['d' + 1] = function() {};
             #p = function() {}; static s = () => 1; static #q = function() {};
             p() { return this.#p.name; }
             static q() { return C.#q.name; }
         }
         const c = new C();
         c.a.name === 'a' && c.b.name === 'b' && c.c.name === 'c' && c.d1.name === 'd1'
             && c.p() === '#p' && C.s.name === 's' && C.q() === '#q'",
    );
    assert_true(
        "const sym = Symbol('desc');
         class C { [sym] = function() {}; static [sym] = class {}; }
         new C()[sym].name === '[desc]' && C[sym].name === '[desc]'",
    );
}

#[test]
fn a_private_element_cannot_be_added_to_the_same_object_twice() {
    assert_true(
        "class Base { constructor(o) { return o; } }
         class Stamper extends Base { #field = 1; static has(o) { return #field in o; } }
         const o = {};
         new Stamper(o);
         let err;
         try { new Stamper(o); } catch (e) { err = e; }
         Stamper.has(o) && err instanceof TypeError",
    );
    assert_true(
        "class Base { constructor(o) { return o; } }
         class Stamper extends Base { #m() {} }
         const o = {};
         new Stamper(o);
         let err;
         try { new Stamper(o); } catch (e) { err = e; }
         err instanceof TypeError",
    );
    // Assigning to a private field that has not been added yet is a TypeError
    // instead of creating it.
    assert_true(
        "class C { a = (this.#b = 1); #b = 2; }
         let err;
         try { new C(); } catch (e) { err = e; }
         err instanceof TypeError",
    );
}

#[test]
fn class_accessors_are_named_get_and_set() {
    assert_true(
        "class C { get id() { return 1; } set id(v) {} static get sid() { return 1; }
                   get #p() { return 1; } static probe() { return 1; } }
         const d = Object.getOwnPropertyDescriptor(C.prototype, 'id');
         const s = Object.getOwnPropertyDescriptor(C, 'sid');
         d.get.name === 'get id' && d.set.name === 'set id' && s.get.name === 'get sid'",
    );
}

#[test]
fn every_super_call_form_initializes_the_fields_of_the_returned_object() {
    // Default derived constructor, explicit super(), spread super(...).
    assert_true(
        "class B { constructor(...args) { this.args = args.join(); } }
         class Default extends B { f = 'default'; }
         class Explicit extends B { f = 'explicit'; constructor(a) { super(a, 2); } }
         class Spread extends B { f = 'spread'; constructor(...rest) { super(...rest); } }
         new Default(1, 2).f === 'default' && new Default(1, 2).args === '1,2'
             && new Explicit(1).f === 'explicit' && new Explicit(1).args === '1,2'
             && new Spread(3, 4).f === 'spread' && new Spread(3, 4).args === '3,4'",
    );
    // Fields see a fully initialized base and run for an arrow's super() too.
    assert_true(
        "class B { constructor() { this.base = true; } }
         class C extends B { seen = this.base; constructor() { const call = () => super(); call(); } }
         new C().seen === true",
    );
    // A base constructor's return override is what gets its fields.
    assert_true(
        "class B { constructor() { return { fromBase: true }; } }
         class C extends B { f = 1; }
         const c = new C();
         c.fromBase === true && c.f === 1 && !(c instanceof C)",
    );
}

#[test]
fn static_blocks_and_fields_run_in_order_with_the_class_as_this() {
    assert_true(
        "const order = [];
         class C {
             static a = order.push('a', this === C);
             static { order.push('block1', this === C); }
             static b = order.push('b');
             static { order.push('block2'); }
             static [(order.push('key'), 'k')] = order.push('k');
         }
         order.join() === 'key,a,true,block1,true,b,block2,k'",
    );
}
