// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Function.prototype.toString` (ECMA-262 §20.2.3.5): a function created from
//! source text returns exactly the text of the syntactic construct that
//! defined it, and any other callable returns a NativeFunction.

use blueice_bluejs::{compile, compile_module, parse, parse_module, RuntimeError, Value, Vm};
use std::collections::HashMap;

/// Evaluates `source` and returns the string it completes with.
fn string_of(source: &str) -> String {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    match Vm::default().execute(&code) {
        Ok(Value::String(text)) => text.to_utf8().unwrap_or_else(|_| {
            panic!("{source}: the result is not well-formed UTF-16");
        }),
        other => panic!("{source}: expected a string, got {other:?}"),
    }
}

fn assert_true(source: &str) {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let code = compile(&program).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    assert_eq!(
        Vm::default().execute(&code),
        Ok(Value::Bool(true)),
        "{source}"
    );
}

/// `text` is the source of a function expression: converting the function it
/// defines to a string gives back exactly `text`.
fn assert_expression_round_trips(text: &str) {
    let source = format!("var f = ({text}\n); f.toString()");
    assert_eq!(string_of(&source), text, "{source}");
    // Both routes to the method agree.
    let source = format!("var f = ({text}\n); Function.prototype.toString.call(f)");
    assert_eq!(string_of(&source), text, "{source}");
}

#[test]
fn every_function_form_returns_its_exact_source_text() {
    for text in [
        "function () {}",
        "function named ( a , b ) { return a + b ; }",
        "function* gen() { yield 1; }",
        "function * spaced ( ) { }",
        "async function af() { await 1; }",
        "async function* ag() { yield 1; }",
        "function /* a */ commented /* b */ ( /* c */ ) /* d */ { /* e */ }",
        "function\nlines\n(\n)\n{\n}",
        "x => x + 1",
        "(a, b) => { return a * b; }",
        "async x => await x",
        "async (a) => { await a; }",
        "(a = (1, 2), { b } = {}, ...c) => 0",
        "x => y => x + y",
        "() => /a/g",
        "() => tag`a${1}b`",
        "class {}",
        "class Named extends Object { constructor() { super(); } }",
        "class  {  static x = 1 ; y ; #z = 2 ; static { } }",
    ] {
        assert_expression_round_trips(text);
    }
}

#[test]
fn function_declarations_and_the_text_around_them() {
    assert_eq!(
        string_of(
            "var before = 1; /* lead */ function decl ( x ) { return x; } // trail\n;\n\
             decl.toString()"
        ),
        "function decl ( x ) { return x; }"
    );
    assert_eq!(
        string_of("async function decl() {}\ndecl.toString()"),
        "async function decl() {}"
    );
    assert_eq!(
        string_of("class Decl  { m() {} }\nDecl.toString()"),
        "class Decl  { m() {} }"
    );
    // A function declared in a block, and its hoisted var binding, are the
    // same function.
    assert_eq!(
        string_of("{ function inBlock() {  } } inBlock.toString()"),
        "function inBlock() {  }"
    );
}

#[test]
fn methods_accessors_and_computed_names_return_their_own_text() {
    let object = "var o = { m ( ) { }, get g ( ) { return 1; }, set s ( v ) { }, *gen ( ) { }, \
                  async am ( ) { }, async * ag ( ) { }, [ 'comp' + 1 ] ( a ) { }, 'q' ( ) { }, \
                  3 ( ) { }, get [ 'ck' ] ( ) { return 1; }, get ( ) { }, async ( ) { } };\n";
    for (access, expected) in [
        ("o.m", "m ( ) { }"),
        (
            "Object.getOwnPropertyDescriptor(o, 'g').get",
            "get g ( ) { return 1; }",
        ),
        (
            "Object.getOwnPropertyDescriptor(o, 's').set",
            "set s ( v ) { }",
        ),
        ("o.gen", "*gen ( ) { }"),
        ("o.am", "async am ( ) { }"),
        ("o.ag", "async * ag ( ) { }"),
        ("o.comp1", "[ 'comp' + 1 ] ( a ) { }"),
        ("o.q", "'q' ( ) { }"),
        ("o[3]", "3 ( ) { }"),
        (
            "Object.getOwnPropertyDescriptor(o, 'ck').get",
            "get [ 'ck' ] ( ) { return 1; }",
        ),
        ("o.get", "get ( ) { }"),
        ("o.async", "async ( ) { }"),
    ] {
        assert_eq!(
            string_of(&format!("{object}{access}.toString()")),
            expected,
            "{access}"
        );
    }
}

#[test]
fn class_methods_exclude_static_and_the_class_constructor_is_the_class() {
    let class = "class C extends Object { constructor ( ) { super ( ) ; } m ( ) { } \
                 static s ( ) { } static async * sag ( ) { } get g ( ) { return 1; } \
                 static set [ 'k' ] ( v ) { } #p ( ) { } static static ( ) { } \
                 static get get ( ) { return 2; } static x = 1 ; y = () => 1 ; }\n";
    let descriptor = |target: &str, key: &str, half: &str| {
        format!("Object.getOwnPropertyDescriptor({target}, '{key}').{half}")
    };
    for (access, expected) in [
        ("C.prototype.m".to_string(), "m ( ) { }"),
        ("C.s".to_string(), "s ( ) { }"),
        ("C.sag".to_string(), "async * sag ( ) { }"),
        (
            descriptor("C.prototype", "g", "get"),
            "get g ( ) { return 1; }",
        ),
        (descriptor("C", "k", "set"), "set [ 'k' ] ( v ) { }"),
        ("C.static".to_string(), "static ( ) { }"),
        (descriptor("C", "get", "get"), "get get ( ) { return 2; }"),
    ] {
        assert_eq!(
            string_of(&format!("{class}{access}.toString()")),
            expected,
            "{access}"
        );
    }
    // The constructor function is the class, so its text is the whole class
    // rather than the `constructor` method.
    let whole = class.trim_end().to_string();
    assert_eq!(string_of(&format!("{class}C.toString()")), whole);
    assert_eq!(
        string_of(&format!("{class}C.prototype.constructor.toString()")),
        whole
    );
    // An arrow function in a field initializer has its own text.
    assert_eq!(
        string_of(&format!("{class}new C().y.toString()")),
        "() => 1"
    );
}

#[test]
fn classes_without_a_constructor_and_derived_classes_return_the_class_text() {
    assert_eq!(string_of("class A { }\nA.toString()"), "class A { }");
    assert_eq!(
        string_of("class A { }\nclass B extends A { x = 1; }\nB.toString()"),
        "class B extends A { x = 1; }"
    );
    assert_eq!(
        string_of("(class  { static #p = 1; }).toString()"),
        "class  { static #p = 1; }"
    );
    assert_eq!(
        string_of("var C = class Inner { static who() { return Inner; } }; C.who().toString()"),
        "class Inner { static who() { return Inner; } }"
    );
}

#[test]
fn a_decorated_class_includes_its_decorators_but_a_decorated_method_does_not() {
    let source = "function dec(value, context) { return value; }\n\
                  @dec class D { @dec m() {} @dec static s() {} }\n";
    assert_eq!(
        string_of(&format!("{source}D.toString()")),
        "@dec class D { @dec m() {} @dec static s() {} }"
    );
    assert_eq!(
        string_of(&format!("{source}D.prototype.m.toString()")),
        "m() {}"
    );
    assert_eq!(string_of(&format!("{source}D.s.toString()")), "s() {}");
    assert_eq!(
        string_of("function dec(v) { return v; }\nvar E = @dec  class { };\nE.toString()"),
        "@dec  class { }"
    );
}

#[test]
fn source_text_is_exact_for_unicode_and_every_line_terminator() {
    assert_eq!(
        string_of("var s = '\u{1F600}'; /* \u{e9} */ function \u{3042}( \u{3044} = '\u{1F600}' ) { }\n\u{3042}.toString()"),
        "function \u{3042}( \u{3044} = '\u{1F600}' ) { }"
    );
    // No normalisation: each terminator is returned as written.
    for terminator in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
        let text = format!("function{terminator}f(a,{terminator}b){terminator}{{{terminator}}}");
        assert_eq!(
            string_of(&format!("{text}{terminator}f.toString()")),
            text,
            "{terminator:?}"
        );
    }
}

#[test]
fn eval_code_has_its_own_source_text() {
    assert_eq!(
        string_of("var f = eval('  (function evaluated ( ) { })  '); f.toString()"),
        "function evaluated ( ) { }"
    );
    assert_eq!(
        string_of("var f = (0, eval)('(x => x + 1)'); f.toString()"),
        "x => x + 1"
    );
    // The declaration eval creates, and a function eval'd from another
    // function's source text, both round-trip.
    assert_eq!(
        string_of("function outer() { eval('function inner() { return 1; }'); return inner; }\nouter().toString()"),
        "function inner() { return 1; }"
    );
    assert_true(
        "function f(x) { return x + 1; }\n\
         var g = eval('(' + f + ')');\n\
         g.toString() === f.toString() && g(1) === 2",
    );
}

#[test]
fn module_code_has_its_own_source_text() {
    let modules: HashMap<_, _> = [
        (
            "main.js",
            "import { f, C } from './dep.js';\n\
             f.toString() === 'function f ( ) { }' && C.toString() === 'class C { m ( ) { } }'\n\
             && (function () { return 1; }).toString() === 'function () { return 1; }'",
        ),
        (
            "dep.js",
            "export function f ( ) { }\nexport class C { m ( ) { } }",
        ),
    ]
    .into_iter()
    .map(|(name, source)| {
        (
            format!("t/{name}"),
            compile_module(&parse_module(source).unwrap()).unwrap(),
        )
    })
    .collect();
    assert_eq!(
        Vm::default().execute_module_graph("t/main.js", &modules),
        Ok(Value::Bool(true))
    );
}

#[test]
fn a_function_nested_in_another_reports_only_its_own_range() {
    assert_true(
        "function outer() { function inner() { return 1; } return inner; }\n\
         outer.toString() === 'function outer() { function inner() { return 1; } return inner; }'\n\
         && outer().toString() === 'function inner() { return 1; }'",
    );
    // Each closure of one function expression has the same text.
    assert_true(
        "function make() { return function ( ) { }; }\n\
         make().toString() === 'function ( ) { }' && make() !== make()",
    );
}

#[test]
fn functions_without_source_text_are_native_functions() {
    // A NativeFunction has the form `function name() { [native code] }`; the
    // exact spelling of the name portion is checked by Test262.
    let native = |expression: &str| {
        let text = string_of(&format!("({expression})"));
        assert!(
            text.starts_with("function") && text.ends_with("() { [native code] }"),
            "{expression} => {text}"
        );
        text
    };
    assert_eq!(
        native("Object.toString.call(Math.max)"),
        "function max() { [native code] }"
    );
    native("Function.prototype.toString.call(function () {}.bind(null))");
    native("Function.prototype.toString.call(new Proxy(function () {}, {}))");
    native("Function.prototype.toString.call(new Proxy(class {}, {}))");
    native("Function.prototype.toString.call(Function.prototype)");
    // A callable that is not a function created from source still works.
    native("Function.prototype.toString.call(Symbol)");
    native("Function.prototype.toString.call(Object.getOwnPropertyDescriptor(Map.prototype, 'size').get)");
}

#[test]
fn a_non_callable_receiver_is_a_type_error() {
    for receiver in [
        "{}",
        "undefined",
        "null",
        "1",
        "'f'",
        "Symbol()",
        "[]",
        "new Proxy({}, {})",
    ] {
        let source = format!("Function.prototype.toString.call({receiver})");
        let program = parse(&source).unwrap();
        let code = compile(&program).unwrap();
        assert!(
            matches!(
                Vm::default().execute(&code),
                Err(RuntimeError::TypeError(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn source_text_is_available_under_gc_stress() {
    let source = "var f = function ( ) { }; var g = class G { m ( ) { } };\n\
                  [f, g, g.prototype.m].map(function (x) { return x.toString(); }).join('|')";
    let program = parse(source).unwrap();
    let code = compile(&program).unwrap();
    let mut config = blueice_bluejs::VmConfig::default();
    config.heap.nursery_capacity = 1;
    let result = blueice_bluejs::Vm::new(config).unwrap().execute(&code);
    assert_eq!(
        result,
        Ok(Value::String(
            "function ( ) { }|class G { m ( ) { } }|m ( ) { }".into()
        ))
    );
}

#[test]
fn dynamic_functions_return_the_source_the_specification_synthesizes() {
    // CreateDynamicFunction: prefix, " anonymous(", the comma-joined
    // parameters, "\n) {", "\n", the body, "\n" and "}".
    for (call, expected) in [
        ("Function()", "function anonymous(\n) {\n\n}"),
        (
            "Function('return 1')",
            "function anonymous(\n) {\nreturn 1\n}",
        ),
        (
            "new Function('a', 'b', 'return a + b;')",
            "function anonymous(a,b\n) {\nreturn a + b;\n}",
        ),
        (
            "Function('a, b', '/* c */ c', 'return a')",
            "function anonymous(a, b,/* c */ c\n) {\nreturn a\n}",
        ),
        // A line comment in the parameters ends at the wrapper's line break.
        (
            "Function('a // c', '')",
            "function anonymous(a // c\n) {\n\n}",
        ),
        // The parameters are stringified, not just accepted as strings.
        (
            "Function({ toString() { return 'x'; } }, 1)",
            "function anonymous(x\n) {\n1\n}",
        ),
        // Annex B HTML-like comments are dropped for parsing but stay in the
        // text the specification prescribes.
        (
            "Function('a <!-- c\\n', 'return a')",
            "function anonymous(a <!-- c\n\n) {\nreturn a\n}",
        ),
    ] {
        assert_eq!(string_of(&format!("{call}.toString()")), expected, "{call}");
    }
    let constructors = "var GeneratorFunction = Object.getPrototypeOf(function* () {}).constructor;\n\
                        var AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;\n\
                        var AsyncGeneratorFunction = Object.getPrototypeOf(async function* () {}).constructor;\n";
    for (call, expected) in [
        (
            "GeneratorFunction('a', 'yield a')",
            "function* anonymous(a\n) {\nyield a\n}",
        ),
        (
            "AsyncFunction('a', 'await a')",
            "async function anonymous(a\n) {\nawait a\n}",
        ),
        (
            "AsyncGeneratorFunction('a', 'yield await a')",
            "async function* anonymous(a\n) {\nyield await a\n}",
        ),
        ("new AsyncFunction()", "async function anonymous(\n) {\n\n}"),
    ] {
        assert_eq!(
            string_of(&format!("{constructors}{call}.toString()")),
            expected,
            "{call}"
        );
    }
    // The text is the function's own, not a nested function's: a function the
    // body creates keeps its own range of the synthesized text.
    assert_eq!(
        string_of("Function('return function inner ( ) { }')().toString()"),
        "function inner ( ) { }"
    );
}

#[test]
fn source_text_edge_cases_around_the_start_of_a_program_and_template_placeholders() {
    // A Hashbang comment, a byte order mark and leading trivia are not part of
    // the text of the function that follows.
    assert_eq!(
        string_of("#!/usr/bin/env node\nfunction f() { }\nf.toString()"),
        "function f() { }"
    );
    assert_eq!(
        string_of("\u{feff}  // c\nfunction f( ) {}\nf.toString()"),
        "function f( ) {}"
    );
    // Identifier escapes are text like any other.
    assert_eq!(
        string_of("function \\u0066( \\u0061 ) { }\nf.toString()"),
        "function \\u0066( \\u0061 ) { }"
    );
    assert_eq!(
        string_of("var o = { \\u0061sync: 1, get \\u0067() { return 1; } };\nObject.getOwnPropertyDescriptor(o, 'g').get.toString()"),
        "get \\u0067() { return 1; }"
    );
    // A function inside a template placeholder, tagged or not, with CRLF in it.
    assert_eq!(
        string_of("`${ function ( \r\n ) { \r\n } }`"),
        "function ( \r\n ) { \r\n }"
    );
    assert_eq!(
        string_of("(function (s, f) { return f.toString(); })`a${ () => \r\n 1 }b`"),
        "() => \r\n 1"
    );
    // One function per evaluation: the text is the same however often it runs.
    assert_true(
        "var fs = []; for (var i = 0; i < 3; i++) fs.push(function () { return i; });\n\
         fs[0] !== fs[1] && fs[0].toString() === fs[2].toString() && fs[1].toString() === 'function () { return i; }'",
    );
}
