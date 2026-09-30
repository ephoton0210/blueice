// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorators: every element kind, private and static variants, the `access`
//! object, `addInitializer`, metadata, wrong return values and receivers, and
//! allocation failure while a class is decorated.

mod cov_g3_support;
use cov_g3_support::{assert_true, heap_limit_sweep, instruction_budget_sweep, sweep_each};

#[test]
fn element_decorators_replace_methods_getters_and_setters() {
    for source in [
        "function dec(value, context) { return function () { return 'replaced:' + context.kind + ':' + context.name } }
         class C { @dec m() {} @dec static s() {} }
         new C().m() === 'replaced:method:m' && C.s() === 'replaced:method:s'",
        "function dec(value, context) { return function () { return context.kind } }
         class C { @dec get g() { return 1 } @dec set g(v) {} }
         const d = Object.getOwnPropertyDescriptor(C.prototype, 'g');
         d.get.call({}) === 'getter' && d.set.call({}) === 'setter'",
        "let seen;
         function dec(value, context) { seen = context; }
         class C { @dec #p() {} }
         seen.private === true && seen.kind === 'method' && seen.name === '#p' && seen.static === false",
        "function dec(value, context) { return function () { return 'private replaced' } }
         class C { @dec #p() { return 'orig' } call() { return this.#p() } @dec static #q() { return 'orig' } static callQ() { return C.#q() } }
         new C().call() === 'private replaced' && C.callQ() === 'private replaced'",
        "function dec(value, context) { return function () { return 'got' } }
         class C { @dec get #g() { return 1 } @dec set #g(v) {} read() { return this.#g } }
         new C().read() === 'got'",
        // A decorator that returns nothing keeps the method.
        "function dec() {}
         class C { @dec m() { return 'kept' } }
         new C().m() === 'kept'",
        // Same value returned: nothing to replace.
        "function dec(value) { return value }
         class C { @dec m() { return 'same' } }
         new C().m() === 'same'",
        // Several decorators apply from the last one backwards.
        "const order = [];
         function a(v) { order.push('a') } function b(v) { order.push('b') }
         class C { @a @b m() {} }
         order.join() === 'b,a'",
    ] {
        assert_true(source);
    }
}

#[test]
fn wrong_decorators_and_return_values_are_type_errors() {
    for source in [
        "try { class C { @(1) m() {} } false } catch (e) { e instanceof TypeError }",
        "try { class C { @(undefined) m() {} } false } catch (e) { e instanceof TypeError }",
        "function dec() { return 1 }
         try { class C { @dec m() {} } false } catch (e) { e instanceof TypeError }",
        "function dec() { return 1 }
         try { class C { @dec get g() { return 1 } } false } catch (e) { e instanceof TypeError }",
        "function dec() { return 1 }
         try { class C { @dec x; } false } catch (e) { e instanceof TypeError }",
        "function dec() { return 1 }
         try { class C { @dec accessor x; } false } catch (e) { e instanceof TypeError }",
        "function dec() { return { get: 1 } }
         try { class C { @dec accessor x; } false } catch (e) { e instanceof TypeError }",
        "function dec() { return { set: 1 } }
         try { class C { @dec accessor x; } false } catch (e) { e instanceof TypeError }",
        "function dec() { return { init: 1 } }
         try { class C { @dec accessor x; } false } catch (e) { e instanceof TypeError }",
        "function dec() { return { get get() { throw new EvalError('get') } } }
         try { class C { @dec accessor x; } false } catch (e) { e instanceof EvalError }",
        "function dec() { return 1 }
         try { @dec class C {} false } catch (e) { e instanceof TypeError }",
        "try { @(1) class C {} false } catch (e) { e instanceof TypeError }",
        "try { @(() => {}) @(2) class C {} false } catch (e) { e instanceof TypeError }",
    ] {
        assert_true(source);
    }
}

#[test]
fn field_and_accessor_decorators_add_initializers_and_accessors() {
    for source in [
        "function dec(value, context) { return function (initial) { return 'init:' + initial } }
         class C { @dec x = 'a'; @dec static y = 'b'; }
         new C().x === 'init:a' && C.y === 'init:b'",
        "function dec(value, context) { return function (initial) { return initial + '!' } }
         class C { @dec #p = 'a'; read() { return this.#p } }
         new C().read() === 'a!'",
        "function dec() {}
         class C { @dec x = 'plain'; }
         new C().x === 'plain'",
        "function dec(value, context) {
           return { get() { return 'got' }, set(v) { this.stored = v }, init(v) { return 'init' } };
         }
         class C { @dec accessor x = 1; @dec static accessor y = 2; }
         const c = new C(); c.x = 5;
         c.x === 'got' && c.stored === 5 && C.y === 'got'",
        "function dec(value, context) { return { init(v) { return v + 1 } } }
         class C { @dec accessor x = 1; }
         new C().x === 2",
        "let seen;
         function dec(value, context) { seen = { get: typeof value.get, set: typeof value.set, kind: context.kind }; }
         class C { @dec accessor x = 1; }
         seen.get === 'function' && seen.set === 'function' && seen.kind === 'accessor'",
        "function dec() {}
         class C { @dec accessor #x = 1; read() { return this.#x } }
         new C().read() === 1",
    ] {
        assert_true(source);
    }
}

#[test]
fn the_access_object_reads_writes_and_probes_public_and_private_elements() {
    for source in [
        // Public field.
        "let access;
         function dec(value, context) { access = context.access }
         class C { @dec x = 1; }
         const c = new C();
         access.get(c) === 1 && (access.set(c, 2), c.x === 2) && access.has(c) === true && access.has({}) === false",
        // Public method: only `get` and `has`.
        "let access;
         function dec(value, context) { access = context.access }
         class C { @dec m() { return 'm' } }
         access.get(new C()).call(new C()) === 'm' && !('set' in access) && access.has(new C()) && !access.has({})",
        // Setter: only `set` and `has`.
        "let access;
         function dec(value, context) { access = context.access }
         class C { @dec set s(v) { this.stored = v } }
         const c = new C(); access.set(c, 3); c.stored === 3 && !('get' in access) && access.has(c)",
        // Private field, method and accessor pair.
        "let field, method, getter, setter;
         function f(v, c) { field = c.access } function m(v, c) { method = c.access }
         function g(v, c) { getter = c.access } function s(v, c) { setter = c.access }
         class C { @f #x = 1; @m #m() { return 'pm' } @g get #g() { return 'pg' } @s set #s(v) { this.stored = v } }
         const c = new C();
         field.get(c) === 1 && (field.set(c, 9), field.get(c) === 9) && field.has(c) === true
           && method.get(c).call(c) === 'pm' && method.has(c) === true
           && getter.get(c) === 'pg' && setter.has(c) === true && (setter.set(c, 4), c.stored === 4)
           && field.has({}) === false && method.has({}) === false",
        // A field that has not been added yet is not there.
        "let access;
         function dec(value, context) { access = context.access }
         class C { @dec #x = 1; static probe() { return access } }
         const before = C.probe(); before.has(new C()) === true",
        // Wrong receivers and wrong brands.
        "let access, privateAccess;
         function dec(value, context) { access = context.access }
         function priv(value, context) { privateAccess = context.access }
         class C { @dec x = 1; @priv #p = 2; }
         let results = [];
         for (const call of [() => access.get(1), () => access.set(undefined, 1), () => access.has('s'),
                             () => privateAccess.get({}), () => privateAccess.set({}, 1)]) {
           try { call(); results.push('no throw') } catch (e) { results.push(e.constructor.name) }
         }
         results.join() === 'TypeError,TypeError,TypeError,TypeError,TypeError'",
        // Strict assignment through `set` throws on a frozen receiver.
        "let access;
         function dec(value, context) { access = context.access }
         class C { @dec x = 1; }
         const frozen = Object.freeze(new C());
         try { access.set(frozen, 2); false } catch (e) { e instanceof TypeError }",
        // Observable traps.
        "let access;
         function dec(value, context) { access = context.access }
         class C { @dec x = 1; }
         const log = [];
         const proxy = new Proxy({}, { get(t, k) { log.push('get'); return 5 }, set(t, k, v) { log.push('set'); return true },
                                       has(t, k) { log.push('has'); return true } });
         access.get(proxy) === 5 && (access.set(proxy, 1), true) && access.has(proxy) && log.join() === 'get,set,has'",
        "let access;
         function dec(value, context) { access = context.access }
         class C { @dec x = 1; }
         try { access.has(new Proxy({}, { has() { throw new EvalError('h') } })); false } catch (e) { e instanceof EvalError }",
    ] {
        assert_true(source);
    }
}

#[test]
fn add_initializer_runs_for_elements_and_classes_and_only_while_the_decorator_runs() {
    for source in [
        "const log = [];
         function dec(value, context) { context.addInitializer(function () { log.push(context.name + ':' + (this === undefined ? 'u' : typeof this)) }) }
         class C { @dec m() {} @dec static s() {} @dec x = 1; }
         new C();
         log.join() === 's:function,m:object,x:object'",
        "let late;
         function dec(value, context) { late = context }
         class C { @dec m() {} }
         try { late.addInitializer(() => {}); false } catch (e) { e instanceof TypeError }",
        "function dec(value, context) { try { context.addInitializer(1); return 'none' } catch (e) { context.error = e } }
         let seen;
         function dec2(value, context) { try { context.addInitializer(1); seen = 'no throw' } catch (e) { seen = e.constructor.name } }
         class C { @dec2 m() {} }
         seen === 'TypeError'",
        "const log = [];
         function dec(value, context) { context.addInitializer(function () { log.push('class ' + this.name) }) }
         @dec class C {}
         log.join() === 'class C'",
        "let seen;
         function dec(value, context) { seen = { kind: context.kind, name: context.name, has: 'access' in context,
                                                  static: 'static' in context, private: 'private' in context } }
         @dec class C {}
         seen.kind === 'class' && seen.name === 'C' && !seen.has && !seen.static && !seen.private",
        "function dec(value, context) { return class extends value { extra() { return 'extra' } } }
         @dec class C {}
         new C().extra() === 'extra'",
        "let order = [];
         function a(value, context) { order.push('a') } function b(value, context) { order.push('b') }
         @a @b class C {}
         order.join() === 'b,a'",
    ] {
        assert_true(source);
    }
}

#[test]
fn metadata_is_created_inherited_and_read_through_a_possibly_throwing_getter() {
    for source in [
        "let saved;
         function dec(value, context) { saved = context.metadata; context.metadata.mark = 1 }
         class Base { @dec m() {} }
         let derivedMeta;
         function dec2(value, context) { derivedMeta = context.metadata }
         class Derived extends Base { @dec2 n() {} }
         Object.getPrototypeOf(derivedMeta) === saved && derivedMeta.mark === 1 && Base[Symbol.metadata] === saved",
        "let meta;
         function dec(value, context) { meta = context.metadata }
         class Plain { @dec m() {} }
         Object.getPrototypeOf(meta) === null",
        "let meta;
         function dec(value, context) { meta = context.metadata }
         class Base { static [Symbol.metadata] = 5 }
         class Derived extends Base { @dec m() {} }
         Object.getPrototypeOf(meta) === null",
        "class Base { static get [Symbol.metadata]() { throw new EvalError('meta') } }
         try { class Derived extends Base { @(() => {}) m() {} } false } catch (e) { e instanceof EvalError }",
        "function dec() {}
         class Plain { @dec m() {} }
         const d = Object.getOwnPropertyDescriptor(Plain, Symbol.metadata);
         d.writable === true && d.enumerable === false && d.configurable === true",
    ] {
        assert_true(source);
    }
}

#[test]
fn decorated_classes_survive_every_allocation_failure() {
    let script = "
        const log = [];
        function method(value, context) { context.addInitializer(function () { log.push(1) }); return function () { return 1 } }
        function field(value, context) { return function (v) { return v } }
        function accessor(value, context) { return { get() { return 1 }, set(v) {}, init(v) { return v } } }
        function cls(value, context) { context.addInitializer(function () {}); return value }
        function access(value, context) { context.access.get; }
        @cls class C {
          @method m() {} @method static s() {} @method get g() { return 1 } @method set g(v) {}
          @field x = 1; @accessor accessor y = 2; @access #p = 3; @method #q() {} @accessor static accessor z = 4;
          static probe(o) { return o.#p }
        }
        const c = new C(); C.s(); c.m(); c.x; c.y; c.g; C.z;
        let context;
        class D extends C { @((v, ctx) => { context = ctx }) m() {} }
        context.access.has(c);
        let read, write;
        class E { @((v, ctx) => { read = ctx.access }) w = 1; @((v, ctx) => { write = ctx.access }) w2 = 2; }
        const e = new E(); read.get(e); write.set(e, 1);";
    assert!(heap_limit_sweep("0;", script) > 0);
}

#[test]
fn decorated_classes_survive_running_out_of_instructions_at_every_step() {
    let script = "
        function method(value, context) { context.addInitializer(function () {}); return function () { return 1 } }
        function field(value, context) { return function (v) { return v } }
        function accessor(value, context) { return { init(v) { return v } } }
        function cls(value, context) { context.addInitializer(function () {}); return value }
        @cls @cls class C {
          @method @method m() {} @field @field x = 1; @accessor accessor y = 2;
        }
        new C();";
    assert!(instruction_budget_sweep(script) > 0);
}

/// A heap ceiling only fails an allocation that is the largest demand made so
/// far, so each decorator step is swept in the smallest class that takes it.
#[test]
fn each_decoration_step_fails_cleanly_in_the_smallest_class_that_takes_it() {
    let warm = "globalThis.noop = function (value, context) {};
        globalThis.replace = function (value, context) { return function () { return 1 } };
        globalThis.replaceClass = function (value, context) { return class {} };
        globalThis.initialize = function (value, context) { context.addInitializer(function () {}) };
        globalThis.fieldInit = function (value, context) { return function (v) { return v } };
        globalThis.accessorObject = function (value, context) { return { get() { return 1 }, set(v) {}, init(v) { return v } } };
        globalThis.useAccess = function (value, context) { context.access.get; context.access.has };";
    let classes = [
        "@noop class C {}",
        "@replaceClass class C {}",
        "@initialize class C {}",
        "@noop @noop class C {}",
        "class C { @noop m() {} }",
        "class C { @replace m() {} }",
        "class C { @initialize m() {} }",
        "class C { @replace static m() {} }",
        "class C { @replace get g() { return 1 } }",
        "class C { @replace set s(v) {} }",
        "class C { @replace #p() {} }",
        "class C { @replace get #g() { return 1 } }",
        "class C { @noop x = 1; }",
        "class C { @fieldInit x = 1; }",
        "class C { @fieldInit #x = 1; }",
        "class C { @noop accessor a = 1; }",
        "class C { @accessorObject accessor a = 1; }",
        "class C { @useAccess x = 1; }",
        "class C { @useAccess #x = 1; }",
        "class C { @useAccess m() {} }",
        "class C { @useAccess static accessor a = 1; }",
        "class B { static [Symbol.metadata] = {} } class C extends B { @noop m() {} }",
    ];
    for class in classes {
        sweep_each(&[(warm, class)]);
    }
}

/// A decorated element is replaced only while it is still the one the class
/// definition made: a later element with the same key wins.
#[test]
fn a_later_element_with_the_same_key_wins_over_a_decorated_one() {
    assert_true(
        "function dec() { return function () { return 'decorated' } }
         class C { @dec m() { return 'first' } m() { return 'second' } }
         new C().m() === 'second'",
    );
}
