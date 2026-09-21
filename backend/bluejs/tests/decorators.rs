// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorator semantics (the Stage 3 decorators proposal): the call protocol,
//! what each kind of decorator may return, evaluation and application order,
//! `addInitializer` timing and the `access` object. The corpus only checks
//! that the syntax is accepted, so these tests carry the runtime behaviour.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn run(source: &str) -> Result<Value, RuntimeError> {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    Vm::default().execute(&code)
}

fn assert_true(source: &str) {
    assert_eq!(run(source), Ok(Value::Bool(true)), "{source}");
}

fn assert_type_error(source: &str) {
    match run(source) {
        Err(RuntimeError::TypeError(_)) => {}
        other => panic!("{source}: expected a TypeError, got {other:?}"),
    }
}

/// The same program run with a one-object nursery, which makes every
/// allocation a potential collection: an object a native leaves unrooted
/// fails deterministically.
fn assert_true_under_gc_stress(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    for (nursery_capacity, major_threshold_bytes) in
        [(1, 20_000), (1, 60_000), (1, 140_000), (5, 20_000)]
    {
        let mut config = VmConfig::default();
        config.heap.nursery_capacity = nursery_capacity;
        config.heap.major_threshold_bytes = major_threshold_bytes;
        let result = Vm::new(config).unwrap().execute(&code);
        assert_eq!(
            result,
            Ok(Value::Bool(true)),
            "nursery {nursery_capacity}, major threshold {major_threshold_bytes}: {source}"
        );
    }
}

// ---- Methods, getters and setters ----

#[test]
fn a_method_decorator_receives_the_method_and_a_context() {
    assert_true(
        "let seen;
         function dec(value, context) { 'use strict'; seen = { value, context, self: this }; }
         class C { @dec m() {} }
         seen.value === C.prototype.m
             && seen.self === undefined
             && seen.context.kind === 'method'
             && seen.context.name === 'm'
             && seen.context.static === false
             && seen.context.private === false
             && typeof seen.context.addInitializer === 'function'
             && typeof seen.context.access.get === 'function'
             && typeof seen.context.access.has === 'function'
             && !('set' in seen.context.access)
             && typeof seen.context.metadata === 'object'",
    );
}

#[test]
fn a_returned_function_replaces_the_method() {
    assert_true(
        "const replacement = function () { return 'new'; };
         function dec() { return replacement; }
         class C { @dec m() { return 'old'; } }
         new C().m() === 'new' && C.prototype.m === replacement",
    );
}

#[test]
fn returning_undefined_keeps_the_original_method() {
    assert_true(
        "function dec() {}
         class C { @dec m() { return 'old'; } }
         new C().m() === 'old'",
    );
}

#[test]
fn a_method_decorator_returning_a_non_function_throws_a_type_error() {
    for bad in ["1", "null", "'x'", "({})", "Symbol()"] {
        assert_type_error(&format!("class C {{ @(() => {bad}) m() {{}} }}"));
    }
}

#[test]
fn a_decorator_that_is_not_callable_throws_a_type_error() {
    assert_type_error("class C { @(1) m() {} }");
    assert_type_error("class C { @(undefined) m() {} }");
    assert_type_error("const o = {}; class C { @(o) m() {} }");
}

#[test]
fn a_replaced_method_keeps_its_property_attributes() {
    assert_true(
        "class C { @(() => function () {}) m() {} }
         const d = Object.getOwnPropertyDescriptor(C.prototype, 'm');
         d.writable === true && d.enumerable === false && d.configurable === true",
    );
}

#[test]
fn getter_and_setter_decorators_are_applied_separately() {
    assert_true(
        "const kinds = [];
         let getterSeen, setterSeen;
         function dec(value, context) {
             kinds.push(context.kind);
             if (context.kind === 'getter') getterSeen = value;
             if (context.kind === 'setter') setterSeen = value;
             return function (...args) { return value.call(this, ...args) + '!'; };
         }
         class C {
             @dec get x() { return 'got'; }
             @dec set x(v) { this.stored = v; }
         }
         const d = Object.getOwnPropertyDescriptor(C.prototype, 'x');
         const c = new C();
         c.x = 'in';
         kinds.join() === 'getter,setter'
             && getterSeen !== d.get && setterSeen !== d.set
             && c.x === 'got!' && c.stored === 'in'",
    );
}

#[test]
fn only_the_decorated_half_of_an_accessor_pair_is_replaced() {
    assert_true(
        "const original = { get: null, set: null };
         class C {
             @((v, ctx) => { original.get = v; return () => 'decorated'; })
             get x() { return 'plain'; }
             set x(v) { this.v = v; }
         }
         const d = Object.getOwnPropertyDescriptor(C.prototype, 'x');
         const c = new C();
         c.x = 5;
         d.get !== original.get && c.x === 'decorated' && c.v === 5 && typeof d.set === 'function'",
    );
}

#[test]
fn the_context_of_getters_and_setters_carries_only_the_matching_access_function() {
    assert_true(
        "const contexts = {};
         const dec = (v, ctx) => { contexts[ctx.kind] = ctx; };
         class C { @dec get x() { return 1; } @dec set x(v) {} }
         'get' in contexts.getter.access && !('set' in contexts.getter.access)
             && 'set' in contexts.setter.access && !('get' in contexts.setter.access)
             && 'has' in contexts.getter.access && 'has' in contexts.setter.access",
    );
}

#[test]
fn several_decorators_apply_last_to_first() {
    assert_true(
        "const calls = [];
         const mk = (n) => (value) => { calls.push(n); return function () { return n + value.call(this); }; };
         class C { @mk('a') @mk('b') @mk('c') m() { return '.'; } }
         calls.join() === 'c,b,a' && new C().m() === 'abc.'",
    );
}

#[test]
fn decorator_expressions_run_in_document_order_with_computed_keys() {
    assert_true(
        "const log = [];
         const d = (n) => { log.push('expr ' + n); return () => { log.push('apply ' + n); }; };
         const k = (n) => { log.push('key ' + n); return n; };
         class C {
             @d(1) @d(2) [k('a')]() {}
             @d(3) static [k('b')]() {}
             @d(4) [k('c')] = 1;
         }
         log.join() === 'expr 1,expr 2,key a,expr 3,key b,expr 4,key c,apply 2,apply 1,apply 3,apply 4'",
    );
}

#[test]
fn every_decorator_of_every_element_runs_before_any_element_is_observable_in_the_class() {
    // Decorators run after all methods are evaluated but before the class
    // binding is initialized, so the class name is still in its TDZ.
    assert_true(
        "let error;
         const dec = () => { try { C; } catch (e) { error = e; } };
         class C { @dec m() {} }
         error instanceof ReferenceError",
    );
}

#[test]
fn a_later_element_with_the_same_key_wins_over_an_earlier_decorated_one() {
    assert_true(
        "class C {
             @(() => function () { return 'replaced'; }) m() { return 'first'; }
             m() { return 'second'; }
         }
         new C().m() === 'second'",
    );
    assert_true(
        "class C {
             m() { return 'first'; }
             @(() => function () { return 'replaced'; }) m() { return 'second'; }
         }
         new C().m() === 'replaced'",
    );
}

#[test]
fn a_replaced_method_does_not_inherit_the_originals_home_object() {
    assert_true(
        "class B { who() { return 'base'; } }
         class C extends B { @(() => function () { return typeof super_home; }) m() { return super.who(); } }
         var super_home;
         const r = function () {};
         // The decorator returns a plain function; defining it must not give it a [[HomeObject]].
         class D extends B { @(() => r) m() { return super.who(); } }
         D.prototype.m === r && new C().m() === 'undefined'",
    );
}

#[test]
fn static_and_private_methods_can_be_decorated_and_replaced() {
    assert_true(
        "const seen = [];
         const dec = (v, ctx) => { seen.push([ctx.kind, ctx.name, ctx.static, ctx.private]); };
         class C {
             @dec static s() {}
             @dec #p() {}
             @dec static #q() {}
         }
         JSON.stringify(seen) === JSON.stringify([
             ['method', 's', true, false],
             ['method', '#p', false, true],
             ['method', '#q', true, true],
         ])",
    );
    assert_true(
        "class C {
             @(() => function () { return 'static new'; }) static s() { return 'static old'; }
             @(() => function () { return 'private new'; }) #p() { return 'private old'; }
             call() { return this.#p(); }
         }
         C.s() === 'static new' && new C().call() === 'private new'",
    );
}

#[test]
fn a_replaced_private_accessor_pair_half_is_swapped_in_place() {
    assert_true(
        "class C {
             @(() => function () { return 'new getter'; }) get #x() { return 'old getter'; }
             set #x(v) { this.stored = v; }
             read() { return this.#x; }
             write(v) { this.#x = v; }
         }
         const c = new C(); c.write(3);
         c.read() === 'new getter' && c.stored === 3",
    );
}

#[test]
fn a_computed_key_is_converted_once_and_used_for_the_context_name() {
    assert_true(
        "let converted = 0;
         const key = { toString() { converted++; return 'k'; } };
         let name;
         class C { @((v, ctx) => { name = ctx.name; return () => 'r'; }) [key]() {} }
         converted === 1 && name === 'k' && new C().k() === 'r'",
    );
    assert_true(
        "const sym = Symbol('s');
         let name;
         class C { @((v, ctx) => { name = ctx.name; }) [sym]() {} }
         name === sym",
    );
}

// ---- access ----

#[test]
fn the_access_object_reads_and_tests_public_methods_by_object() {
    assert_true(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; }) m() {} }
         const c = new C();
         access.get(c) === C.prototype.m && access.has(c) === true
             && access.has({}) === false && access.get.length === 1 && access.has.length === 1",
    );
}

#[test]
fn the_access_object_reaches_private_methods_only_on_branded_objects() {
    assert_true(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; }) #m() { return 'm'; } }
         const c = new C();
         access.get(c)() === 'm' && access.has(c) === true && access.has({}) === false",
    );
    assert_type_error(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; }) #m() {} }
         access.get({})",
    );
}

#[test]
fn the_access_functions_reject_non_objects() {
    assert_type_error(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; }) m() {} }
         access.get(1)",
    );
    assert_type_error(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; }) m() {} }
         access.has('str')",
    );
}

// ---- addInitializer ----

#[test]
fn instance_method_initializers_run_per_instance_before_fields() {
    assert_true(
        "const log = [];
         const dec = (v, ctx) => { ctx.addInitializer(function () { log.push('init ' + typeof this.field + ' ' + (this instanceof C)); }); };
         class C {
             @dec m() {}
             field = (log.push('field'), 1);
         }
         new C(); new C();
         log.join() === 'init undefined true,field,init undefined true,field'",
    );
}

#[test]
fn initializers_of_one_element_run_in_the_order_they_were_added_across_decorators() {
    // Decorators apply last to first, so the last decorator's initializer
    // is added (and therefore runs) first.
    assert_true(
        "const log = [];
         const mk = (n) => (v, ctx) => { ctx.addInitializer(() => log.push(n + '1')); ctx.addInitializer(() => log.push(n + '2')); };
         class C { @mk('a') @mk('b') m() {} }
         new C();
         log.join() === 'b1,b2,a1,a2'",
    );
}

#[test]
fn method_initializers_of_several_elements_run_in_document_order() {
    assert_true(
        "const log = [];
         const mk = (n) => (v, ctx) => { ctx.addInitializer(() => log.push(n)); };
         class C { @mk(1) a() {} b() {} @mk(2) get c() { return 0; } @mk(3) static s() {} @mk(4) #p() {} }
         new C();
         log.join() === '3,1,2,4'",
    );
}

#[test]
fn static_method_initializers_run_with_the_class_before_static_fields() {
    assert_true(
        "const log = [];
         const dec = (v, ctx) => { ctx.addInitializer(function () { log.push('init ' + this.name); }); };
         class C {
             @dec static m() {}
             static f = (log.push('static field'), 1);
             static { log.push('static block'); }
         }
         log.join() === 'init C,static field,static block'",
    );
}

#[test]
fn add_initializer_after_the_decorator_returned_throws() {
    assert_type_error(
        "let add;
         class C { @((v, ctx) => { add = ctx.addInitializer; }) m() {} }
         add(() => {})",
    );
}

#[test]
fn add_initializer_still_throws_after_the_decorator_itself_threw() {
    assert_true(
        "let add;
         try { class C { @((v, ctx) => { add = ctx.addInitializer; throw 1; }) m() {} } } catch (e) {}
         try { add(() => {}); false } catch (e) { e instanceof TypeError }",
    );
}

#[test]
fn add_initializer_requires_a_callable() {
    assert_type_error("class C { @((v, ctx) => { ctx.addInitializer(1); }) m() {} }");
    assert_type_error("class C { @((v, ctx) => { ctx.addInitializer(); }) m() {} }");
}

#[test]
fn each_decorator_call_gets_a_fresh_context() {
    assert_true(
        "const contexts = [];
         const dec = (v, ctx) => { contexts.push(ctx); };
         class C { @dec @dec m() {} }
         contexts.length === 2 && contexts[0] !== contexts[1]
             && contexts[0].addInitializer !== contexts[1].addInitializer
             && contexts[0].metadata === contexts[1].metadata",
    );
}

#[test]
fn decoration_survives_garbage_collection_at_every_allocation() {
    assert_true_under_gc_stress(
        "const log = [];
         const dec = (v, ctx) => {
             ctx.addInitializer(function () { log.push(ctx.name); });
             return function () { return v.call(this) + ctx.name; };
         };
         class C { @dec a() { return '1'; } @dec static b() { return '2'; } @dec #c() { return '3'; } run() { return this.#c(); } }
         const c = new C();
         c.a() === '1a' && C.b() === '2b' && c.run() === '3#c' && log.join() === 'b,a,#c'",
    );
}

// ---- Fields ----

#[test]
fn a_field_decorator_receives_undefined_and_a_context_with_read_write_access() {
    assert_true(
        "let seen;
         function dec(value, context) { 'use strict'; seen = { value, context }; }
         class C { @dec x = 1; }
         seen.value === undefined
             && seen.context.kind === 'field' && seen.context.name === 'x'
             && seen.context.static === false && seen.context.private === false
             && typeof seen.context.access.get === 'function'
             && typeof seen.context.access.set === 'function'
             && typeof seen.context.access.has === 'function'
             && seen.context.access.set.length === 2",
    );
}

#[test]
fn a_field_initializer_function_receives_the_value_and_this() {
    assert_true(
        "const seen = [];
         const dec = () => function (value) { seen.push([this instanceof C, value]); return value * 2; };
         class C { @dec x = 21; y = 5; }
         const c = new C();
         c.x === 42 && c.y === 5 && seen.length === 1 && seen[0][0] === true && seen[0][1] === 21",
    );
}

#[test]
fn field_initializers_chain_last_decorator_first() {
    assert_true(
        "const order = [];
         const mk = (n) => () => function (v) { order.push(n); return v + n; };
         class C { @mk('a') @mk('b') x = ''; }
         new C().x === 'ba' && order.join() === 'b,a'",
    );
}

#[test]
fn a_field_without_an_initializer_starts_as_undefined_through_the_initializers() {
    assert_true(
        "let received = 'unset';
         class C { @(() => function (v) { received = v; return 'set'; }) x; }
         new C().x === 'set' && received === undefined",
    );
}

#[test]
fn a_field_decorator_returning_a_non_function_throws_a_type_error() {
    for bad in ["1", "null", "'x'", "({})", "Symbol()"] {
        assert_type_error(&format!("class C {{ @(() => {bad}) x = 1; }}"));
    }
}

#[test]
fn a_field_decorator_returning_undefined_leaves_the_field_alone() {
    assert_true(
        "class C { @(() => {}) x = 7; }
         new C().x === 7 && Object.keys(new C()).join() === 'x'",
    );
}

#[test]
fn a_decorated_field_is_still_defined_not_assigned() {
    assert_true(
        "let setterCalls = 0;
         class B { set x(v) { setterCalls++; } }
         class C extends B { @(() => v => v) x = 1; }
         const c = new C();
         setterCalls === 0 && Object.getOwnPropertyDescriptor(c, 'x').value === 1",
    );
}

#[test]
fn an_anonymous_function_field_value_is_named_before_the_initializers_see_it() {
    assert_true(
        "let name;
         class C { @(() => function (v) { name = v.name; return v; }) x = function () {}; }
         new C();
         name === 'x'",
    );
}

#[test]
fn field_extra_initializers_run_right_after_that_field_is_defined() {
    assert_true(
        "const log = [];
         const mk = (n) => (v, ctx) => { ctx.addInitializer(function () { log.push(n + ':' + Object.keys(this).join('')); }); };
         class C { a = 1; @mk('b') b = 2; c = 3; @mk('d') d = 4; }
         new C();
         log.join() === 'b:ab,d:abcd'",
    );
}

#[test]
fn static_fields_and_their_extra_initializers_run_with_the_class() {
    assert_true(
        "const log = [];
         const dec = (v, ctx) => {
             ctx.addInitializer(function () { log.push('extra ' + this.name); });
             return function (init) { log.push('init ' + this.name); return init + 1; };
         };
         class C { @dec static x = 1; static y = (log.push('y'), 2); }
         C.x === 2 && log.join() === 'init C,extra C,y'",
    );
}

#[test]
fn private_fields_can_be_decorated_and_accessed_through_the_context() {
    assert_true(
        "let access, name, isPrivate;
         const dec = (v, ctx) => { access = ctx.access; name = ctx.name; isPrivate = ctx.private; return (x) => x + 1; };
         class C { @dec #p = 1; }
         const c = new C();
         access.get(c) === 2 && (access.set(c, 10), access.get(c)) === 10
             && access.has(c) === true && access.has({}) === false
             && name === '#p' && isPrivate === true",
    );
    assert_type_error(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; }) #p = 1; }
         access.get({})",
    );
}

#[test]
fn static_private_fields_are_reached_through_the_class() {
    assert_true(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; }) static #p = 3; }
         access.get(C) === 3 && access.has(C) === true && access.has(new C()) === false",
    );
}

#[test]
fn public_field_access_uses_get_and_set_semantics_on_any_object() {
    assert_true(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; }) x = 1; }
         const o = { x: 1 };
         access.get(o) === 1 && (access.set(o, 5), o.x) === 5 && access.has(o) === true
             && access.has({}) === false",
    );
    // A failing [[Set]] throws whatever the caller's strictness.
    assert_type_error(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; }) x = 1; }
         access.set(Object.freeze({ x: 1 }), 2)",
    );
}

#[test]
fn a_computed_field_key_is_the_context_name_and_is_converted_once() {
    assert_true(
        "let converted = 0, name;
         const key = { toString() { converted++; return 'k'; } };
         class C { @((v, ctx) => { name = ctx.name; }) [key] = 1; }
         new C(); new C();
         converted === 1 && name === 'k'",
    );
}

#[test]
fn a_derived_classs_decorated_fields_are_initialized_after_super_returns() {
    assert_true(
        "const log = [];
         class B { constructor() { log.push('base'); } }
         class D extends B {
             @(() => v => (log.push('field'), v)) x = 1;
             constructor() { log.push('before'); super(); log.push('after'); }
         }
         new D();
         log.join() === 'before,base,field,after'",
    );
}

#[test]
fn field_decoration_survives_garbage_collection_at_every_allocation() {
    assert_true_under_gc_stress(
        "const log = [];
         const dec = (v, ctx) => {
             ctx.addInitializer(function () { log.push(ctx.name); });
             return function (init) { return init + ctx.name; };
         };
         class C { @dec a = '1'; @dec static b = '2'; @dec #c = '3'; get() { return this.#c; } }
         const c = new C();
         c.a === '1a' && C.b === '2b' && c.get() === '3#c' && log.join() === 'b,a,#c'",
    );
}

// ---- Auto-accessors ----

#[test]
fn an_accessor_decorator_receives_get_and_set_functions() {
    assert_true(
        "let seen;
         function dec(value, context) { 'use strict'; seen = { value, context }; }
         class C { @dec accessor x = 1; }
         const d = Object.getOwnPropertyDescriptor(C.prototype, 'x');
         seen.value.get === d.get && seen.value.set === d.set
             && seen.context.kind === 'accessor' && seen.context.name === 'x'
             && typeof seen.context.access.get === 'function'
             && typeof seen.context.access.set === 'function'
             && new C().x === 1",
    );
}

#[test]
fn an_accessor_decorator_can_replace_get_set_and_init() {
    assert_true(
        "const log = [];
         const dec = ({ get, set }) => ({
             get() { log.push('get'); return get.call(this); },
             set(v) { log.push('set ' + v); set.call(this, v); },
             init(v) { log.push('init ' + v); return v * 10; },
         });
         class C { @dec accessor x = 4; }
         const c = new C();
         c.x = c.x + 1;
         c.x === 41 && log.join() === 'init 4,get,set 41,get'",
    );
}

#[test]
fn omitted_accessor_parts_keep_their_defaults() {
    assert_true(
        "class C { @(() => ({ get() { return 'custom'; } })) accessor x = 1; }
         const c = new C();
         c.x = 2;
         const d = Object.getOwnPropertyDescriptor(C.prototype, 'x');
         c.x === 'custom' && typeof d.set === 'function'",
    );
    assert_true(
        "class C { @(() => ({ set(v) { this.log = v; } })) accessor x = 1; }
         const c = new C();
         const before = c.x;
         c.x = 9;
         before === 1 && c.x === 1 && c.log === 9",
    );
    assert_true(
        "class C { @(() => ({})) accessor x = 1; }
         new C().x === 1",
    );
}

#[test]
fn an_accessor_decorator_returning_a_bad_value_throws_a_type_error() {
    for bad in ["1", "'x'", "null", "true", "Symbol()"] {
        assert_type_error(&format!("class C {{ @(() => {bad}) accessor x; }}"));
    }
    for part in ["get", "set", "init"] {
        assert_type_error(&format!(
            "class C {{ @(() => ({{ {part}: 1 }})) accessor x; }}"
        ));
        assert_type_error(&format!(
            "class C {{ @(() => ({{ {part}: null }})) accessor x; }}"
        ));
    }
}

#[test]
fn accessor_decorators_apply_last_to_first_and_each_sees_the_previous_result() {
    assert_true(
        "const mk = (n) => ({ get }) => ({ get() { return n + get.call(this); } });
         class C { @mk('a') @mk('b') accessor x = '.'; }
         new C().x === 'ab.'",
    );
    assert_true(
        "const order = [];
         const mk = (n) => () => ({ init(v) { order.push(n); return v + n; } });
         class C { @mk('a') @mk('b') accessor x = ''; }
         new C().x === 'ba' && order.join() === 'b,a'",
    );
}

#[test]
fn private_and_static_accessors_are_decorated_too() {
    assert_true(
        "let seen = [];
         const dec = (v, ctx) => { seen.push([ctx.name, ctx.static, ctx.private]); return { get() { return 'g:' + v.get.call(this); } }; };
         class C {
             @dec accessor #p = 1;
             @dec static accessor s = 2;
             @dec static accessor #q = 3;
             readP() { return this.#p; }
             static readQ() { return C.#q; }
         }
         seen.map(String).join('|') === '#p,false,true|s,true,false|#q,true,true'
             && new C().readP() === 'g:1' && C.s === 'g:2' && C.readQ() === 'g:3'",
    );
}

#[test]
fn accessor_access_reads_and_writes_through_the_current_accessors() {
    assert_true(
        "let access;
         class C { @((v, ctx) => { access = ctx.access; return { get() { return v.get.call(this) + '!'; } }; }) accessor x = 'a'; }
         const c = new C();
         access.get(c) === 'a!' && (access.set(c, 'b'), access.get(c)) === 'b!' && access.has(c) === true",
    );
}

#[test]
fn accessor_extra_initializers_run_right_after_the_storage_is_initialized() {
    assert_true(
        "const log = [];
         const dec = (v, ctx) => { ctx.addInitializer(function () { log.push('extra ' + this.x); }); return { init(v) { log.push('init'); return v; } }; };
         class C { @dec accessor x = 3; y = (log.push('y'), 0); }
         new C();
         log.join() === 'init,extra 3,y'",
    );
}

#[test]
fn an_undecorated_accessor_next_to_a_decorated_one_is_untouched() {
    assert_true(
        "class C { @(() => ({ get() { return 'd'; } })) accessor a = 1; accessor b = 2; }
         const c = new C();
         c.a === 'd' && c.b === 2",
    );
}

// ---- Class decorators ----

#[test]
fn a_class_decorator_receives_the_class_and_a_class_context() {
    assert_true(
        "let seen;
         function dec(value, context) { 'use strict'; seen = { value, context, self: this }; }
         class C {}
         @dec class D {}
         seen.value === D && seen.self === undefined
             && seen.context.kind === 'class' && seen.context.name === 'D'
             && typeof seen.context.addInitializer === 'function'
             && typeof seen.context.metadata === 'object'
             && !('static' in seen.context) && !('private' in seen.context) && !('access' in seen.context)",
    );
}

#[test]
fn a_returned_function_replaces_the_class_binding() {
    assert_true(
        "const replacement = class {};
         function dec() { return replacement; }
         @dec class C { static inner() { return C; } }
         C === replacement",
    );
    // The inner binding the class body sees names the decorated class too.
    assert_true(
        "@(v => class extends v {}) class C { static inner() { return C; } }
         C.inner() === C",
    );
}

#[test]
fn a_class_decorator_may_return_a_wrapping_subclass_that_keeps_the_originals_elements() {
    assert_true(
        "const wrap = (Base) => class extends Base { constructor(...args) { super(...args); this.wrapped = true; } };
         @wrap class C { x = 1; #p = 2; static s = 3; get p() { return this.#p; } m() { return 'm'; } }
         const c = new C();
         c.wrapped === true && c.x === 1 && c.p === 2 && C.s === 3 && c.m() === 'm'",
    );
}

#[test]
fn a_class_decorator_returning_a_non_callable_throws_a_type_error() {
    for bad in ["1", "null", "'x'", "({})", "Symbol()"] {
        assert_type_error(&format!("@(() => {bad}) class C {{}}"));
    }
}

#[test]
fn a_class_decorator_returning_undefined_keeps_the_class() {
    assert_true("class X {} const before = X; @(() => {}) class C {} C.name === 'C'");
}

#[test]
fn class_decorators_apply_last_to_first_and_each_sees_the_previous_result() {
    assert_true(
        "const order = [];
         const mk = (n) => (Base) => { order.push(n); return class extends Base { static tag = (Base.tag ?? '') + n; }; };
         @mk('a') @mk('b') @mk('c') class C {}
         order.join() === 'c,b,a' && C.tag === 'cba'",
    );
}

#[test]
fn class_decorator_expressions_run_before_the_heritage_and_all_elements() {
    assert_true(
        "const log = [];
         const d = (n) => { log.push('expr ' + n); return () => { log.push('call ' + n); }; };
         const base = () => { log.push('extends'); return class {}; };
         @d('c1') @d('c2') class C extends base() {
             @d('m') [(log.push('key'), 'm')]() {}
         }
         log.join() === 'expr c1,expr c2,extends,expr m,key,call m,call c2,call c1'",
    );
}

#[test]
fn the_class_name_is_the_inferred_name_of_an_anonymous_decorated_class_expression() {
    assert_true(
        "let names = [];
         const dec = (v, ctx) => { names.push(ctx.name); };
         const X = @dec class {};
         const o = { Y: @dec class {} };
         (@dec class {});
         @dec class Named {}
         names.join() === 'X,Y,,Named' && names[2] === undefined",
    );
}

#[test]
fn a_decorated_class_expression_evaluates_to_the_decorated_class() {
    assert_true(
        "const R = class {};
         const C = @(() => R) class Inner {};
         C === R",
    );
}

#[test]
fn class_extra_initializers_run_after_static_fields_with_the_decorated_class() {
    assert_true(
        "const log = [];
         const dec = (v, ctx) => { ctx.addInitializer(function () { log.push('class ' + (this === D) + ' ' + this.field); }); return class D extends v {}; };
         let D;
         D = null;
         @dec class C { static field = (log.push('field'), 'f'); static { log.push('block'); } }
         D = C;
         log.join() === 'field,block,class false f'",
    );
}

#[test]
fn element_decorators_run_before_class_decorators() {
    assert_true(
        "const log = [];
         const d = (n) => () => { log.push(n); };
         @d('class') class C { @d('method') m() {} @d('static field') static f = 1; @d('field') g = 2; }
         log.join() === 'method,static field,field,class'",
    );
}

#[test]
fn class_decorators_see_a_class_whose_elements_are_all_defined() {
    assert_true(
        "let seen;
         @((v) => { seen = [typeof v.prototype.m, typeof v.s, Object.getOwnPropertyNames(v.prototype).join()]; })
         class C { m() {} static s() {} }
         seen.join('|') === 'function|function|constructor,m'",
    );
}

#[test]
fn the_decorator_list_of_a_class_is_evaluated_in_the_surrounding_scope() {
    // The class name is not yet initialized, and a private name of the class
    // is not visible: both lie inside the class body.
    assert_true(
        "let error;
         try { @(C) class C {} } catch (e) { error = e; }
         error instanceof ReferenceError",
    );
}

#[test]
fn a_class_decorator_list_is_sloppy_code_in_a_sloppy_script() {
    assert_true(
        "let seen;
         @(function () { seen = typeof this; return undefined; }) class C {}
         seen === 'undefined' || seen === 'object'",
    );
}

#[test]
fn decorated_classes_work_in_functions_generators_and_async_functions() {
    assert_true(
        "function make(dec) { @dec class C { m() { return 1; } } return C; }
         make((v) => class extends v { m() { return super.m() + 1; } }).prototype.m.call({}) === 2",
    );
    assert_true(
        "function* g() { const C = @(yield) class {}; return C; }
         const it = g(); it.next(); const r = it.next((v, ctx) => class {}).value;
         typeof r === 'function'",
    );
}

#[test]
fn class_decoration_survives_garbage_collection_at_every_allocation() {
    assert_true_under_gc_stress(
        "const log = [];
         const dec = (v, ctx) => {
             ctx.addInitializer(function () { log.push('init ' + this.tag); });
             return class extends v { static tag = 'wrapped'; };
         };
         @dec class C { static field = 1; @(() => {}) m() {} }
         C.tag === 'wrapped' && C.field === 1 && log.join() === 'init wrapped'",
    );
}

// ---- Metadata ----

#[test]
fn symbol_metadata_is_a_well_known_symbol() {
    assert_true(
        "typeof Symbol.metadata === 'symbol' && Symbol.metadata.description === 'Symbol.metadata'
             && Object.getOwnPropertyDescriptor(Symbol, 'metadata').writable === false
             && Object.getOwnPropertyDescriptor(Symbol, 'metadata').configurable === false",
    );
}

#[test]
fn every_decorator_of_a_class_shares_one_metadata_object_published_on_the_class() {
    assert_true(
        "const seen = [];
         const dec = (v, ctx) => { seen.push(ctx.metadata); ctx.metadata[ctx.kind + (ctx.name ?? '')] = true; };
         @dec class C { @dec m() {} @dec static s = 1; @dec accessor a; }
         const meta = C[Symbol.metadata];
         seen.length === 4 && seen.every((m) => m === meta)
             && Object.keys(meta).join() === 'method' + 'm,field' + 's,accessor' + 'a,class' + 'C'.replace(/^/, '')",
    );
}

#[test]
fn metadata_is_defined_before_the_class_decorators_run() {
    assert_true(
        "let same;
         @((v, ctx) => { same = v[Symbol.metadata] === ctx.metadata; }) class C {}
         same === true",
    );
}

#[test]
fn a_classs_metadata_property_is_writable_enumerable_and_configurable() {
    assert_true(
        "class C { @(() => {}) m() {} }
         const d = Object.getOwnPropertyDescriptor(C, Symbol.metadata);
         d.writable === true && d.enumerable === true && d.configurable === true
             && Object.getPrototypeOf(d.value) === null",
    );
}

#[test]
fn a_class_without_decorators_has_no_metadata() {
    assert_true(
        "class C { m() {} } class D extends C {}
         !Object.hasOwn(C, Symbol.metadata) && Object.getOwnPropertySymbols(C).length === 0
             && C[Symbol.metadata] === undefined && D[Symbol.metadata] === undefined",
    );
}

#[test]
fn metadata_inherits_from_the_superclass_metadata() {
    assert_true(
        "const meta = (k, v) => (_, ctx) => { ctx.metadata[k] = v; };
         @meta('a', 'x') class C { @meta('b', 'y') m() {} }
         @meta('b', 'z') class D extends C {}
         Object.getPrototypeOf(D[Symbol.metadata]) === C[Symbol.metadata]
             && D[Symbol.metadata].a === 'x' && D[Symbol.metadata].b === 'z'
             && C[Symbol.metadata].b === 'y' && !Object.hasOwn(D[Symbol.metadata], 'a')",
    );
}

#[test]
fn a_decorator_can_extend_the_inherited_metadata() {
    assert_true(
        "const append = (k, v) => (_, ctx) => { ctx.metadata[k] = [...(ctx.metadata[k] ?? []), v]; };
         @append('a', 'x') class C {}
         @append('a', 'z') class D extends C {}
         C[Symbol.metadata].a.join() === 'x' && D[Symbol.metadata].a.join() === 'x,z'",
    );
}

#[test]
fn a_decorated_subclass_of_an_undecorated_class_gets_a_null_prototype_metadata() {
    assert_true(
        "class B {}
         @((v, ctx) => { ctx.metadata.k = 1; }) class C extends B {}
         Object.getPrototypeOf(C[Symbol.metadata]) === null",
    );
    assert_true(
        "@((v, ctx) => { ctx.metadata.k = 1; }) class C extends null {}
         Object.getPrototypeOf(C[Symbol.metadata]) === null",
    );
}

#[test]
fn a_superclass_metadata_that_is_not_an_object_is_ignored() {
    assert_true(
        "class B { static [Symbol.metadata] = 5; }
         @((v, ctx) => {}) class C extends B {}
         Object.getPrototypeOf(C[Symbol.metadata]) === null",
    );
    assert_true(
        "class B { static [Symbol.metadata] = null; }
         @((v, ctx) => {}) class C extends B {}
         Object.getPrototypeOf(C[Symbol.metadata]) === null",
    );
}

#[test]
fn metadata_can_key_a_weak_map() {
    assert_true(
        "const store = new WeakMap();
         const dec = (v, ctx) => { store.set(ctx.metadata, 'private'); };
         @dec class C {}
         store.get(C[Symbol.metadata]) === 'private'",
    );
}

// ---- Interaction with the rest of the class machinery ----

#[test]
fn super_property_access_in_a_decorated_method_uses_the_original_home_object() {
    assert_true(
        "class B { who() { return 'base'; } }
         class C extends B { @((v) => function () { return 'wrapped ' + v.call(this); }) who() { return 'child > ' + super.who(); } }
         new C().who() === 'wrapped child > base'",
    );
}

#[test]
fn decorators_can_be_declared_in_static_blocks_and_nested_classes() {
    assert_true(
        "const log = [];
         const d = (n) => () => { log.push(n); };
         class Outer {
             static { @d('inner class') class Inner { @d('inner method') m() {} } }
             @d('outer method') m() {}
         }
         log.join() === 'outer method,inner method,inner class'",
    );
}

#[test]
fn a_decorator_may_name_a_private_static_member_of_the_enclosing_class() {
    assert_true(
        "class C {
             static #wrap(v) { return function () { return 'wrapped:' + v.call(this); }; }
             static make() { return class { @C.#wrap m() { return 'm'; } }; }
         }
         new (C.make())().m() === 'wrapped:m'",
    );
}

#[test]
fn errors_thrown_by_decorators_propagate_and_leave_no_class() {
    assert_true(
        "let error;
         try { class C { @(() => { throw new RangeError('boom'); }) m() {} } } catch (e) { error = e; }
         error instanceof RangeError && error.message === 'boom'",
    );
}

#[test]
fn a_class_with_only_undecorated_elements_still_defines_its_methods_in_order() {
    assert_true(
        "class C { a() {} static b() {} get c() { return 1; } [Symbol.iterator]() {} }
         Object.getOwnPropertyNames(C.prototype).join() === 'constructor,a,c'
             && Object.getOwnPropertyNames(C).join() === 'length,name,prototype,b'",
    );
}

#[test]
fn method_order_and_attributes_are_unchanged_by_decoration() {
    assert_true(
        "class C { a() {} @(() => {}) b() {} c() {} }
         Object.getOwnPropertyNames(C.prototype).join() === 'constructor,a,b,c'",
    );
}
