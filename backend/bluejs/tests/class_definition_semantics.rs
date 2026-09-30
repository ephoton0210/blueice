// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ClassDefinitionEvaluation regressions found by the Test262 class scope:
//! heritage, constructor property order, inner name bindings, and the derived
//! constructor `this`/`new.target` environment.

use blueice_bluejs::{compile, parse, Value, Vm};

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let result = Vm::default().execute(&code);
    assert_eq!(result, Ok(Value::Bool(true)), "{source}");
}

#[test]
fn extends_null_keeps_function_prototype_as_the_constructor_parent() {
    assert_true(
        "class Foo extends null {}
         Object.getPrototypeOf(Foo.prototype) === null
             && Object.getPrototypeOf(Foo) === Function.prototype
             && Foo.prototype.constructor === Foo",
    );
    assert_true(
        "const E = class extends null { constructor() {} };
         Object.getPrototypeOf(E) === Function.prototype
             && Object.getPrototypeOf(E.prototype) === null",
    );
}

#[test]
fn class_constructor_own_keys_start_with_length_name_prototype() {
    assert_true(
        "class C { static m() {} static a = 1; static [Symbol.iterator]() {} }
         Object.getOwnPropertyNames(C).join() === 'length,name,prototype,m,a'",
    );
    assert_true(
        "const C = class { static m() {} };
         Object.getOwnPropertyNames(C).join() === 'length,name,prototype,m'",
    );
    assert_true(
        "class C { static [1]() {} static ['x']() {} }
         Object.getOwnPropertyNames(C).join() === '1,length,name,prototype,x'",
    );
}

#[test]
fn an_arrow_function_closes_over_its_creators_new_target() {
    assert_true(
        "function F() { return () => new.target; }
         const viaNew = new F();
         const viaCall = F();
         viaNew() === F && viaCall() === undefined",
    );
    // The caller's own `new.target` never leaks into the arrow.
    assert_true(
        "function F() { return () => new.target; }
         const arrow = F();
         let seen = 'unset';
         function G() { seen = arrow(); }
         new G();
         seen === undefined",
    );
    assert_true(
        "function F() { return () => () => new.target; }
         new F()()() === F",
    );
    assert_true(
        "function F() { return () => eval('new.target'); }
         new F()() === F",
    );
}

#[test]
fn a_derived_constructors_this_is_shared_with_its_arrows_and_eval() {
    assert_true(
        "class B {}
         let probe;
         class C extends B {
             constructor() {
                 probe = () => this;
                 let before;
                 try { probe(); } catch (e) { before = e; }
                 if (!(before instanceof ReferenceError)) throw new Error('no TDZ');
                 super();
             }
         }
         const c = new C();
         probe() === c",
    );
    assert_true(
        "class B {}
         class C extends B {
             constructor() {
                 const bind = () => super();
                 let before;
                 try { this; } catch (e) { before = e; }
                 bind();
                 if (!(before instanceof ReferenceError)) throw new Error('no TDZ');
                 this.ok = eval('this') === this && (() => this)() === this;
             }
         }
         new C().ok",
    );
    assert_true(
        "class B {}
         class C extends B { constructor() { eval('super()'); } }
         new C() instanceof C",
    );
}

#[test]
fn super_call_binds_this_once_after_constructing() {
    assert_true(
        "let built = 0;
         class B { constructor() { built++; } }
         class C extends B {
             constructor() {
                 super();
                 let err;
                 try { super(); } catch (e) { err = e; }
                 this.err = err;
             }
         }
         const c = new C();
         built === 2 && c.err instanceof ReferenceError",
    );
    // The super constructor is fetched before the arguments are evaluated and
    // checked for IsConstructor only afterwards.
    assert_true(
        "let evaluated = false;
         class C extends Object {
             constructor() {
                 try { super(evaluated = true); } catch (e) { this_err = e; }
             }
         }
         var this_err;
         Object.setPrototypeOf(C, parseInt);
         try { new C(); } catch (e) {}
         evaluated && this_err instanceof TypeError",
    );
    assert_true(
        "class C extends Object { constructor() { super[super()]; } }
         let err;
         try { new C(); } catch (e) { err = e; }
         err instanceof ReferenceError",
    );
}

#[test]
fn a_derived_constructor_that_never_binds_this_throws_a_reference_error() {
    assert_true(
        "class B {}
         class C extends B { constructor() {} }
         let err;
         try { new C(); } catch (e) { err = e; }
         err instanceof ReferenceError",
    );
    assert_true(
        "class B {}
         class C extends B { constructor() { return 1; } }
         let err;
         try { new C(); } catch (e) { err = e; }
         err instanceof TypeError",
    );
    assert_true(
        "class B {}
         class C extends B { constructor() { return {x: 1}; } }
         new C().x === 1",
    );
}

#[test]
fn a_super_property_captures_its_base_before_the_key_or_value_runs() {
    assert_true(
        "const proto = { p: 'ok' }, proto2 = { p: 'bad' };
         const key = { toString() { Object.setPrototypeOf(obj, proto2); return 'p'; } };
         const obj = { __proto__: proto, m() { return super[key]; } };
         obj.m() === 'ok'",
    );
    assert_true(
        "const proto = { p: 1 }, proto2 = { p: -1 };
         const key = { toString() { Object.setPrototypeOf(obj, proto2); return 'p'; } };
         const obj = { __proto__: proto, m() { return super[key] += 1; } };
         obj.m() === 2",
    );
    assert_true(
        "let calls = 0;
         const key = { toString() { calls++; return 'p'; } };
         const obj = { __proto__: { p: 1 }, m() { super[key] += 1; super[key]++; } };
         obj.m();
         calls === 2 && obj.p === 2",
    );
    // The base is fixed before the right-hand side runs.
    assert_true(
        "const proto = { set prop(v) { this.assigned = v; } };
         class D { m() { super.prop = (Object.setPrototypeOf(D.prototype, null), 7); } }
         Object.setPrototypeOf(D.prototype, proto);
         const d = new D();
         d.m();
         d.assigned === 7",
    );
    assert_true(
        "class D { static m() { super[0] = count += 1; } }
         var count = 0;
         Object.setPrototypeOf(D, null);
         let err;
         try { D.m(); } catch (e) { err = e; }
         err instanceof TypeError && count === 1",
    );
}

#[test]
fn super_properties_are_valid_destructuring_for_and_delete_operands() {
    assert_true(
        "let log = [];
         class B { set x(v) { log.push('x', v); } set y(v) { log.push('y', v); } }
         class C extends B {
             m() {
                 [super.x] = [1];
                 ({ a: super.y } = { a: 2 });
                 for (super.x of [3]) {}
                 for (super.y in { k: 0 }) {}
                 [...super.x] = [4, 5];
             }
         }
         new C().m();
         log.join() === 'x,1,y,2,x,3,y,k,x,4,5'",
    );
    assert_true(
        "class B {}
         class C extends B {
             m() {
                 let out = [];
                 try { delete super.x; } catch (e) { out.push(e instanceof ReferenceError); }
                 try { delete super[(out.push('key'), 'x')]; } catch (e) { out.push(e instanceof ReferenceError); }
                 return out.join();
             }
         }
         new C().m() === 'true,key,true'",
    );
    assert_true(
        "class B { get t() { return function(s) { return s[0] + this.tag; }; } }
         class C extends B { constructor() { super(); this.tag = '!'; } m() { return super.t`a`; } }
         new C().m() === 'a!'",
    );
}

#[test]
fn a_generator_body_sees_undefined_new_target_and_may_eval_it() {
    assert_true(
        "function* g() {
             yield new.target;
             yield eval('new.target');
             yield (() => new.target)();
         }
         const it = g();
         it.next().value === undefined && it.next().value === undefined && it.next().value === undefined",
    );
}

#[test]
fn functions_in_a_class_heritage_or_computed_key_are_strict() {
    // The class is defined in sloppy code, but all of it is strict code.
    assert_true(
        "var D = class extends function() { arguments.callee; } {};
         let heritageThrows = false, constructThrows = false;
         try { Object.getPrototypeOf(D).arguments; } catch (e) { heritageThrows = e instanceof TypeError; }
         try { new D; } catch (e) { constructThrows = e instanceof TypeError; }
         heritageThrows && constructThrows",
    );
    assert_true(
        "class C { [(function() { return typeof this; })()]() {} }
         Object.getOwnPropertyNames(C.prototype).includes('undefined')",
    );
}
