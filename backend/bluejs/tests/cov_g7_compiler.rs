// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class, decorator and function compilation: the bytecode-size limit is
//! lowered through every value up to what the program needs, so each emit
//! site of the class/function compiler fails in turn, plus the compile-time
//! early errors of class private names.

use blueice_bluejs::{
    compile, compile_with_limit, parse, ClassElement, CompileError, PropertyKey, Stmt, Value, Vm,
};

/// Every class-element kind, decorated and not, private and public, static and
/// instance, with computed keys, static blocks, derived constructors and
/// `arguments` objects.
const CLASSES: &str = r#"
var log = [];
function deco(value, context) { log.push(context.kind); return value; }
function key() { return 'k' + log.length; }
@deco @deco class Base {
  static #sp = 1;
  static #sm() { return 1 }
  static get #sg() { return 2 }
  static set #sg(v) {}
  #ip = 2;
  #im() { return 3 }
  get #ig() { return 4 }
  set #ig(v) {}
  [key()] = 5;
  static [key()] = 6;
  static { log.push('block') }
  @deco method() {}
  @deco static smethod() {}
  @deco get acc() { return 1 }
  @deco set acc(v) {}
  @deco field = 1;
  @deco static sfield = 2;
  @deco accessor auto = 3;
  @deco static accessor sauto = 4;
  @deco #pm() {}
  @deco static #spm() {}
  @deco #pf = 1;
  @deco accessor #pa = 2;
  @deco [key()]() {}
  @deco static [key()] = 1;
  static async *gen() {}
  async m2() {}
  *g2() {}
  ['computed']() {}
  static accessor plain = 1;
  read() { return this.#ip + this.#im() + this.#ig + Base.#sp + Base.#sm() + Base.#sg }
}
class Derived extends Base { constructor() { super(); this.a = 1 } }
class Fields { a; b; static c; #d; static #e; }
class Named { static n = Named.name; static { Named.done = true } }
var Anonymous = class { static #p = 1; static has(o) { return #p in o } };
var expr = class Inner extends Base { static who() { return Inner } };
class OnlyElements {
  @deco m() {}
  @deco static s = 1;
  @deco get [key()]() { return 1 }
  @deco static set [key()](v) {}
  @deco static get [key()]() { return 1 }
  @deco set [key()](v) {}
}
var holder;
var PrivHost = class Host { static #d(v) { return v } static { holder = Host } static Inner = class { @holder.#d m() {} } };
var opt = { d(v) { return v } };
class OptionalDecorator { @(opt?.d) m() {} }
class MemberDecorator { @opt.d m() {} }
class ComputedAutoAccessors { accessor [key()] = 5; static accessor [key()] = 6; }
function paramEval(a = eval('1')) { var x = 1; return a }
function paramVar(a, b = 1) { var a; return a }
function* genParams(a = 1) { var a; yield a }
function withArguments(a, b) { return arguments[0] + a }
function duplicateParameters(a, a) { return arguments[0] }
function withDefaults(a, b = 1, ...rest) { return arguments.length }
function withDuplicate(a, a2) { arguments[0] = 9; return a }
var named = function self() { return self };
var arrow = (x, y = 2) => x + y;
var fnExpr = function () { return new.target };
async function af() { await 1; for await (var x of []) {} }
function* gen() { yield 1; yield* [2]; }
var obj = { get a() { return 1 }, set a(v) {}, m() {}, async *n() {}, ['c' + 1]: 2 };
new Derived().read();
"#;

#[test]
fn the_class_program_runs() {
    let mut vm = Vm::default();
    let value = vm
        .execute(&compile(&parse(CLASSES).unwrap()).unwrap())
        .unwrap();
    // #ip + #im() + #ig + #sp + #sm() + #sg
    assert_eq!(value, Value::Number(2.0 + 3.0 + 4.0 + 1.0 + 1.0 + 2.0));
}

#[test]
fn every_emit_site_fails_when_the_bytecode_limit_is_too_small() {
    let program = parse(CLASSES).unwrap();
    // The smallest sufficient limit, by doubling then bisecting.
    let mut high = 1024u32;
    while compile_with_limit(&program, high).is_err() {
        high *= 2;
    }
    let mut low = high / 2;
    while high - low > 1 {
        let middle = (low + high) / 2;
        if compile_with_limit(&program, middle).is_ok() {
            high = middle;
        } else {
            low = middle;
        }
    }
    for limit in 0..high {
        assert_eq!(
            compile_with_limit(&program, limit).err(),
            Some(CompileError::ProgramTooLarge),
            "limit {limit}"
        );
    }
    assert!(compile_with_limit(&program, high).is_ok());
}

#[test]
fn class_private_name_early_errors_are_compile_errors() {
    for source in [
        "class C { #a; #a }",
        "class C { get #a() {} get #a() {} }",
        "class C { static #a; #a }",
        "class C { #a; static #a() {} }",
        "class C { m() { this.#missing } }",
        "class C { m() { #missing in this } }",
        "class C { static #a; m() { class D { n() { this.#b } } } }",
    ] {
        let parsed = parse(source);
        match parsed {
            Err(_) => {}
            Ok(program) => {
                assert!(compile(&program).is_err(), "{source}");
            }
        }
    }
}

#[test]
fn duplicate_private_names_in_a_constructed_class_are_rejected_by_the_compiler() {
    // The parser refuses duplicates itself, so mutate a parsed program.
    let mut program = parse("class C { #a = 1; #b = 2 }").unwrap();
    let Stmt::ClassDecl(class) = &mut program.body[0] else {
        panic!("a class declaration parses to Stmt::ClassDecl");
    };
    let ClassElement::Field { key, .. } = &mut class.elements[1] else {
        panic!("a field parses to ClassElement::Field");
    };
    *key = PropertyKey::Identifier("#a".into());
    assert_eq!(
        compile(&program).err(),
        Some(CompileError::InvalidSyntax(
            "duplicate private name in class body"
        ))
    );
}

#[test]
fn valid_private_name_reuse_across_nested_classes_compiles() {
    for source in [
        "class C { #a = 1; m() { class D { #a = 2; n() { return this.#a } } return new D().n() } }",
        "class C { get #a() { return 1 } set #a(v) {} m() { this.#a = this.#a } }",
        "class C { static #a() {} static m() { return C.#a } }",
    ] {
        assert!(compile(&parse(source).unwrap()).is_ok(), "{source}");
    }
}
