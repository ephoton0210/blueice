// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Sweeps over one corpus of expression forms. Every bytecode limit below the
//! size a program needs must be rejected with `ProgramTooLarge` (never partial
//! bytecode), so each emit/constant site in the expression compiler is
//! exercised as the instruction that crosses the budget; and every prefix of
//! every source must be either accepted or rejected with a described parse
//! error, so each expected-token check in the expression parser is exercised
//! as the point where the input ends.

use blueice_bluejs::{
    compile, compile_module_with_limit, compile_with_limit, parse, parse_module, CompileError,
};

fn sweep_script(source: &str) {
    let program = parse(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    // Some sources are deliberately rejected (early errors); their rejection
    // must still be reported once the budget no longer interferes.
    let full = compile(&program);
    let mut settled = false;
    // Nested functions are budgeted too, so the needed limit can exceed the
    // top-level code alone.
    for limit in 0..1 << 16 {
        let bounded = compile_with_limit(&program, limit);
        if bounded.as_ref().err() == Some(&CompileError::ProgramTooLarge) {
            continue;
        }
        assert_eq!(
            bounded
                .as_ref()
                .map(|code| code.bytes().to_vec())
                .map_err(Clone::clone),
            full.as_ref()
                .map(|code| code.bytes().to_vec())
                .map_err(Clone::clone),
            "{source}"
        );
        settled = true;
        break;
    }
    assert!(settled, "{source}");
}

fn sweep_module(source: &str) {
    let module = parse_module(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    let mut succeeded = false;
    for limit in 0..4096 {
        match compile_module_with_limit(&module, limit) {
            Ok(_) => {
                succeeded = true;
                break;
            }
            Err(error) => assert_eq!(error, CompileError::ProgramTooLarge, "{source}, {limit}"),
        }
    }
    assert!(succeeded, "{source}");
}

#[test]
fn expression_forms_report_the_budget_at_every_instruction() {
    for source in SCRIPTS {
        sweep_script(source);
    }
}

#[test]
fn every_prefix_of_every_source_parses_or_reports_a_described_error() {
    for source in SCRIPTS.iter().chain(MODULES) {
        assert!(
            parse(source).is_ok() || parse_module(source).is_ok(),
            "{source}"
        );
        for end in 0..source.len() {
            if !source.is_char_boundary(end) {
                continue;
            }
            let prefix = &source[..end];
            for result in [parse(prefix).map(|_| ()), parse_module(prefix).map(|_| ())] {
                if let Err(error) = result {
                    assert!(!error.message.is_empty(), "{prefix}");
                }
            }
        }
    }
}

#[test]
fn malformed_sources_report_the_exact_parse_error() {
    for (source, message) in INVALID {
        assert_eq!(
            parse(source).expect_err(source).message,
            *message,
            "{source}"
        );
    }
}

#[test]
fn new_cannot_take_an_await_expression_in_modules() {
    assert_eq!(
        parse_module("new await x").expect_err("new await").message,
        "await cannot immediately follow new (found Identifier(\"await\"))"
    );
    assert!(parse_module("new (await x)").is_ok());
}

#[test]
fn module_expression_forms_report_the_budget_at_every_instruction() {
    for source in MODULES {
        sweep_module(source);
    }
}

/// Sources the parser must reject, with the exact message.
const INVALID: &[(&str, &str)] = &[
    ("[a b] = c", "expected Comma (found Identifier(\"b\"))"),
    ("[+] = c", "expected an expression (found Punct(Plus))"),
    ("[a = +] = c", "expected an expression (found Punct(RBracket))"),
    ("[...+] = c", "expected an expression (found Punct(Plus))"),
    ("({ a b } = c)", "expected Comma (found Identifier(\"b\"))"),
    ("({ + } = c)", "expected a property key (found Punct(Plus))"),
    ("({ a: + } = c)", "expected an expression (found Punct(Plus))"),
    ("({ a: b = + } = c)", "expected an expression (found Punct(RBrace))"),
    ("({ a = + } = c)", "expected an expression (found Punct(RBrace))"),
    ("({...+} = c)", "expected an expression (found Punct(Plus))"),
    ("new a[b c", "expected RBracket (found Identifier(\"c\"))"),
    ("new a`t${", "unterminated or invalid template placeholder"),
    ("f`a${+}b`", "unterminated or invalid template placeholder"),
    ("new a`t${+}b`", "unterminated or invalid template placeholder"),
    ("[a, b c] = d", "expected Comma (found Identifier(\"c\"))"),
    (
        "[...a b] = c",
        "a rest element must be last in a destructuring assignment pattern (found Identifier(\"b\"))",
    ),
    (
        "[...a, b] = c",
        "a rest element must be last in a destructuring assignment pattern (found Punct(Comma))",
    ),
    (
        "({ ...a, b } = c)",
        "a rest property must be last in a destructuring assignment pattern (found Punct(Comma))",
    ),
    ("({ get a(){} } = c)", "expected Comma (found Identifier(\"a\"))"),
    (
        "({ a: 1 } = c)",
        "invalid destructuring assignment target (found Punct(RBrace))",
    ),
    (
        "({ 1 } = c)",
        "destructuring assignment shorthand requires an IdentifierReference (found Punct(RBrace))",
    ),
    (
        "import('a' 'b')",
        "an import call accepts at most two arguments (found String(JsString([98])))",
    ),
    (
        "import(...a)",
        "an import call does not accept a spread argument (found Punct(Ellipsis))",
    ),
    ("import.foo", "unknown import phase (found Identifier(\"foo\"))"),
    ("a?.[b c]", "expected RBracket (found Identifier(\"c\"))"),
    (
        "({ 'a' } = c)",
        "destructuring assignment shorthand requires an IdentifierReference (found Punct(RBrace))",
    ),
    (
        "a?.`t`",
        "an optional chain cannot be used as a template tag (found Template { quasis: [JsString([116])], raw_expressions: [] })",
    ),
    (
        "a?.b`t`",
        "an optional chain cannot be used as a template tag (found Template { quasis: [JsString([116])], raw_expressions: [] })",
    ),
    (
        "async function f() { aw\\u0061it x }",
        "the await keyword cannot contain an escape (found Identifier(\"await\"))",
    ),
    ("new new a[+]", "expected an expression (found Punct(RBracket))"),
    ("new a[+]", "expected an expression (found Punct(RBracket))"),
    ("({ *a: 1 })", "invalid object method (found Number(1.0))"),
    (
        "({ *get a(){} })",
        "invalid object accessor (found Identifier(\"a\"))",
    ),
    ("f`${a b}`", "unterminated or invalid template placeholder"),
    (
        "`${a b}`",
        "unterminated or invalid template placeholder (found Invalid(\"unterminated or invalid template placeholder\"))",
    ),
    ("new f`${a b}`", "unterminated or invalid template placeholder"),
    (
        "async function h() { f`${await}` }",
        "await requires an operand (found Eof)",
    ),
    (
        "function f() { 'use strict'; return '\\01' }",
        "legacy octal and non-octal decimal literals and escapes are not valid in strict mode (found String(JsString([1])))",
    ),
    ("a?.(b c)", "expected Comma (found Identifier(\"c\"))"),
];

const MODULES: &[&str] = &["import.meta.url;", "export default import.meta;"];

const SCRIPTS: &[&str] = &[
    "a?.b",
    "a?.[b]",
    "a?.()",
    "a?.b.c(1)",
    "a?.b()",
    "a?.[0]()",
    "a.b?.()",
    "a?.b.c",
    "a?.b?.c",
    "(a?.b).c",
    "/a+/gi",
    "class B{} class A extends B { m(){ return super.x`t`; } }",
    "a.b`t`",
    "f`t`",
    "f`a${1}b${2}c`",
    "function f(){ 'use strict'; return g`t`; }",
    "function f(){ 'use strict'; return g(1); }",
    "function f(){ 'use strict'; return o.m(1); }",
    "1n",
    "{ let a = 1; a; }",
    "function f(){ let a = 1; return a; }",
    "with(o){ a }",
    "undefined; NaN; Infinity",
    "foo",
    "function f(){ return foo }",
    "with(o){ foo }",
    "typeof foo",
    "typeof undefined",
    "{ let a; typeof a }",
    "with(o){ typeof a }",
    "with(o){ typeof a; }",
    "delete foo",
    "delete o.p",
    "delete o[k]",
    "delete o?.p",
    "delete o?.[k]",
    "delete (o.p)",
    "delete 1",
    "delete (1, 2)",
    "delete this",
    "class B{} class A extends B { m(){ delete super.x } }",
    "class B{} class A extends B { m(){ delete super[k()] } }",
    "class B{} class A extends B { constructor(){ delete super.x } }",
    "class B{} class A extends B { constructor(){ delete super[k()] } }",
    "with(o){ delete a }",
    "function f(){ var a; with(o){ delete a } }",
    "function f(a){ with(o){ delete a } }",
    "{ let a; delete a }",
    "function f(){ var a; delete a }",
    "function f(){ eval('var a'); delete a }",
    "void 0",
    "void f()",
    "-a; +a; !a; ~a; typeof a",
    "a in b",
    "1 + 2 * 3 - 4 / 5 % 6 ** 7",
    "a << 1 >> 2 >>> 3",
    "a < b > c <= d >= e == f != g === h !== i",
    "a & b | c ^ d",
    "a && b",
    "a || b",
    "a ?? b",
    "a ? b : c",
    "a, b, c",
    "[]",
    "[1, , 2]",
    "[...a, b]",
    "[a, ...b]",
    "[,]",
    "[...a, , b]",
    "({})",
    "({ a: 1, b })",
    "({ ...a })",
    "({ ...a, b: 1 })",
    "({ [k]: 1 })",
    "({ 'x': 1 })",
    "({ 1: 1 })",
    "({ get a(){ return 1 } })",
    "({ set a(v){} })",
    "({ get [k](){ return 1 } })",
    "({ a(){} })",
    "({ [k](){} })",
    "({ async a(){} })",
    "({ *a(){} })",
    "({ async *a(){} })",
    "({ __proto__: null })",
    "({ __proto__: a, b: 1 })",
    "({ a: function(){} })",
    "({ a: () => 1 })",
    "({ a: class {} })",
    "({ [k]: function(){} })",
    "({ [k]: () => 1 })",
    "({ 'a': function(){} })",
    "({ 1: function(){} })",
    "({ a: (function(){}) })",
    "class A { m(){ return super.x } }",
    "class A { m(){ return super[k] } }",
    "class A { m(){ return super.x() } }",
    "class A { m(){ return super[k]() } }",
    "class A { m(){ return super.x(...a) } }",
    "class A { static m(){ return super.x = 1 } }",
    "class A { m(){ super.x = 1; super[k] = 2; super.x += 1; super[k] += 1; super.x ||= 1; super[k] &&= 1; super.x ??= 1 } }",
    "class A { m(){ super.x++; --super.x; super[k]++; --super[k] } }",
    "class B{} class A extends B { constructor(){ super.x++; super[k]--; super.y = 1; super[k] = 1; super.z += 1; super[k] += 1 } }",
    "class B{} class A extends B { constructor(){ super.x; super[k]; super.x(); super[k](); } }",
    "class A { #p = 1; m(o){ return #p in o } }",
    "class A { #p = 1; m(){ return this.#p } }",
    "class A { #p = 1; m(o){ return o?.#p } }",
    "class A { #p = 1; m(o){ return o?.a.#p } }",
    "class A { #p = 1; m(o){ return o.#p } }",
    "class A { #p = 1; m(){ return this.#p() } }",
    "class A { #p = 1; m(o){ return o?.#p() } }",
    "class A { #p = 1; m(o){ return o?.a.#p() } }",
    "class A { #p = 1; m(o){ return (o?.#p)() } }",
    "class A { #p = 1; m(o){ return (o.#p)() } }",
    "class A { #p = 1; m(){ this.#p = 2; this.#p += 2; this.#p ||= 1; this.#p &&= 1; this.#p ??= 1 } }",
    "class A { #p = 1; m(){ this.#p++; --this.#p } }",
    "class A { #p = 1; m(){ [this.#p] = [1]; ({ a: this.#p } = {}); [...this.#p] = [1] } }",
    "class A { #p = 1; m(){ for (this.#p of []); for (this.#p in {}); } }",
    "class A { #p = 1; m(){ delete this.#p } }",
    "class A { #m(){} n(){ return this.#m`t` } }",
    "class B{} class A extends B { m(){ return super.m?.() } }",
    "class B{} class A extends B { m(){ return super.m?.(1, ...a) } }",
    "class B{} class A extends B { m(){ return super[k]?.() } }",
    "class B{} class A extends B { m(){ return super.x?.y } }",
    "class A { #p; m(o){ return o?.a.#p } }",
    "class A { #p; m(o){ return o?.a.#p() } }",
    "class A { #p; m(o){ return o?.#p?.() } }",
    "class A { #p; m(o){ return (o?.a.#p)() } }",
    "class A { #p; m(o){ return (o?.[k])() } }",
    "class A { #p; m(o){ return (o?.a)`t` } }",
    "with(o){ let a = 1; a }",
    "with(o){ let a; typeof a }",
    "with(o){ let a; a = 2; a += 1; a++ }",
    "function f(){ with(o){ var a; a; delete a; typeof a } }",
    "with(o){ typeof undefined; typeof Object; typeof zzz }",
    "typeof zzz; typeof Object; typeof undefined",
    "f?.()()",
    "f?.().g",
    "f?.().g()",
    "a?.b.c.d",
    "a?.b().c",
    "a.b?.c.d",
    "(a?.b).c",
    "a?.[b]?.[c]",
    "a?.b?.()",
    "a?.b.c?.()",
    "(a?.b)?.()",
    "a?.b(...c)",
    "a?.(...b)",
    "new a.b`t`",
    "[...a, ...b]",
    "({ ...a, ...b })",
    "class A { static m(){ return super.x``; } }",
    "class B{} class A extends B { constructor(){ super(...a, ...b) } }",
    "class B{} class A extends B { m(){ return (super.x)() } }",
    "f(class {})",
    "[class {}]",
    "a, class {}",
    "x ? class {} : 1",
    "(f)()",
    "(f)?.()",
    "class B{} class A extends B { m(){ for (super.x of []); for (super.x in {}); } }",
    "for (let a of b) { var a }",
    "for (let [a, a] of b);",
    "for (const a in b) { var a }",
    "for (const a of b) { }",
    "for (var a in b) { }",
    "class A { static { for (using a of b); } }",
    "async function f(){ for (await using a of b) { } }",
    "function f(){ for (using a of b) { let g = () => a } }",
    "(a)",
    "(a.b)()",
    "(a?.b)()",
    "(a?.[b])()",
    "(a.b)`t`",
    "a.b()",
    "a[b]()",
    "a.b(...c)",
    "a(...b)",
    "new A",
    "new A(1)",
    "new A(...b)",
    "new a.b(1)",
    "new (a?.b)",
    "eval('1')",
    "eval(...a)",
    "with(o){ f() }",
    "with(o){ f(1) }",
    "with(o){ f(...a) }",
    "with(o){ a = 1 }",
    "with(o){ a += 1 }",
    "with(o){ a++ }",
    "with(o){ a ||= 1 }",
    "with(o){ a = function(){} }",
    "with(o){ [a] = [1] }",
    "with(o){ ({ a } = {}) }",
    "with(o){ for (a of []); }",
    "with(o){ delete o.a; typeof a; }",
    "this",
    "function f(){ return new.target }",
    "class A{}",
    "(class {})",
    "(class A { static x = 1 })",
    "function* g(){ yield }",
    "function* g(){ yield 1 }",
    "function* g(){ yield* a }",
    "function* g(){ yield yield 1 }",
    "async function* g(){ yield* a }",
    "async function* g(){ yield 1 }",
    "async function* g(){ yield }",
    "async function* g(){ await a; yield await b }",
    "async function f(){ await a }",
    "async function f(){ await (a, b) }",
    "import('a')",
    "import('a', {})",
    "import.defer('a')",
    "import.source('a')",
    "import('a', b)",
    "`a`",
    "`a${b}c`",
    "`${a}${b}`",
    "`a${b}c${d}e`",
    "a = 1",
    "a += 1",
    "a -= 1; a *= 1; a /= 1; a %= 1; a **= 1; a <<= 1; a >>= 1; a >>>= 1; a &= 1; a |= 1; a ^= 1",
    "a ||= 1",
    "a &&= 1",
    "a ??= 1",
    "a = function(){}",
    "a = () => 1",
    "a = class {}",
    "a ||= function(){}",
    "a &&= () => 1",
    "a ??= class {}",
    "a.b = 1",
    "a.b += 1",
    "a.b ||= 1",
    "a.b &&= 1",
    "a.b ??= 1",
    "a[b] = 1",
    "a[b] += 1",
    "a[b] ||= 1",
    "a.b = function(){}",
    "{ let a; a = 1; a += 1; a ||= 1; a &&= 1; a ??= 1; a = function(){} }",
    "function f(){ let a; a = 1; a += 1; a ||= 1; a = () => 1; a++; --a; }",
    "function f(){ 'use strict'; foo = 1 }",
    "function f(){ foo += 1 }",
    "function f(){ foo ||= 1 }",
    "function f(){ 'use strict'; foo ||= 1 }",
    "function f(){ 'use strict'; foo += 1; foo++ }",
    "function f(){ 'use strict'; foo = function(){} }",
    "foo++",
    "--foo",
    "a.b++",
    "--a.b",
    "a[b]++",
    "--a[b]",
    "{ let a; a++; --a }",
    "[a, b] = [1, 2]",
    "[a, ...b] = [1, 2]",
    "[a = 1, [b], { c }] = []",
    "[a.b, c[d]] = []",
    "[a.b = 1, c[d] = 2] = []",
    "[...a.b] = []",
    "[...a[b]] = []",
    "[...[a, b]] = []",
    "[...{ length }] = []",
    "[, a] = []",
    "[a, , b] = []",
    "[a.b, ...c.d] = []",
    "({ a, b: c } = {})",
    "({ a = 1 } = {})",
    "({ a: b = 1 } = {})",
    "({ a: b.c } = {})",
    "({ a: b.c = 1 } = {})",
    "({ a: b[c] } = {})",
    "({ [k]: a } = {})",
    "({ [k]: a.b } = {})",
    "({ ...a } = {})",
    "({ ...a.b } = {})",
    "({ a, ...b } = {})",
    "({ a: [b] } = {})",
    "({ a: { b } } = {})",
    "({ 'a': b, 1: c } = {})",
    "class B{} class A extends B { m(){ [super.x] = []; ({ a: super.x } = {}); [super[k]] = []; ({ a: super[k] } = {}); [...super.x] = []; } }",
    "class A { #p; m(){ [this.#p] = []; ({ a: this.#p } = {}); [...this.#p] = []; [this.#p = 1] = []; ({ a: this.#p = 1 } = {}); } }",
    "for (var a of b);",
    "for (var a in b);",
    "for (let a of b);",
    "for (const a in b);",
    "for (a of b);",
    "for (a in b);",
    "for (a.b of c);",
    "for (a.b in c);",
    "for ([a] of b);",
    "for ({ a } of b);",
    "for (var [a] of b);",
    "for (let { a } in b);",
    "for (var a = 1 in b);",
    "for (let a of b) { let c; break; }",
    "for (let a of b) { continue; }",
    "for (let a of b) { let f = () => a; }",
    "for (let a in b) { let f = () => a; }",
    "async function f(){ for await (a of b); }",
    "async function f(){ for await (var a of b); }",
    "async function f(){ for await (let a of b) { let f = () => a; } }",
    "async function f(){ for await (a.b of c); }",
    "async function f(){ for await (using a of b); }",
    "async function f(){ for (await using a of b); }",
    "function f(){ for (using a of b); }",
    "for (f() of b);",
    "for (f() in b);",
    "f() = 1",
    "f() += 1",
    "f()++",
    "++f()",
    "class A { static { this.x = 1 } }",
    "function f(a = 1, [b] = [], { c } = {}, ...d){ return a + b + c + d }",
    "(a = 1, [b], { c }, ...d) => a + b + c + d",
    "async (a) => await a",
    "async () => { await 1 }",
    "() => ({})",
    "() => { return 1 }",
    "x => x",
    "function f(){ return arguments }",
    "function f(){ return () => arguments }",
    "function f(){ return () => this }",
    "function f(){ return () => new.target }",
    "class B{} class A extends B { constructor(){ super() } }",
    "class B{} class A extends B { constructor(){ super(1, 2) } }",
    "class B{} class A extends B { constructor(){ super(...a) } }",
    "class B{} class A extends B { constructor(){ super(...a, 1) } }",
    "class B{} class A extends B { constructor(){ (() => super())() } }",
    "class B{} class A extends B { constructor(){ (() => this)() } }",
    "class B{} class A extends B { constructor(){ this } }",
    "class B{} class A extends B { constructor(){ eval('super()') } }",
    "class B{} class A extends B { x = super.y; static z = super.w; constructor(){ super() } }",
    "class A { static #p = 1; static m(){ return A.#p } }",
    "class A { get #p(){ return 1 } set #p(v){} m(){ return this.#p } }",
    "class A { #m(){} n(){ return this.#m() } }",
    "class A { static async *#m(){} }",
    "class A { accessor x = 1; static accessor y }",
    "@d class A {}",
    "@d() @e.f class A { @g m(){} @h static x = 1 }",
    "(@d class {})",
    "function f(){ 'use strict'; with_(1); return h(1, 2) }",
    "function f(){ 'use strict'; return new.target }",
    "function f(){ 'use strict'; return a.b(...c) }",
    "function f(){ return f`t` }",
    "let { a, b: [c] } = {}",
    "const { a = 1, ...b } = {}",
    "var [a, [b], ...c] = []",
    "using a = b",
    "async function f(){ await using a = b }",
    "label: { break label }",
    "if (a) b; else c",
    "while (a) b",
    "do a; while (b)",
    "for (;;) break",
    "for (let i = 0; i < 1; i++) { let f = () => i }",
    "switch (a) { case 1: b; default: c }",
    "try { a } catch (e) { b } finally { c }",
    "try { a } catch { b }",
    "throw a",
    "debugger",
    "a\n?.b",
    "new.target",
    "async function f(){ new.target }",
];

/// Every token sequence over a small alphabet of the punctuation that shapes
/// destructuring patterns, literals and calls must parse or fail with a
/// described error (never panic), including the malformed inputs whose
/// look-ahead classification and real parse disagree.
#[test]
fn small_token_alphabet_never_panics_the_parser() {
    const ALPHABET: &[&str] = &[
        "[", "]", "{", "}", "a", ",", "=", ":", ".", "...", "+", "(", ")", "1",
    ];
    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut indices = Vec::new();
    fn visit(indices: &mut Vec<usize>, depth: usize, accepted: &mut usize, rejected: &mut usize) {
        if !indices.is_empty() {
            let source = indices
                .iter()
                .map(|&index| ALPHABET[index])
                .collect::<Vec<_>>()
                .join(" ");
            for wrapped in [
                source.clone(),
                format!("({source})"),
                format!("x = {source}"),
            ] {
                match parse(&wrapped) {
                    Ok(_) => *accepted += 1,
                    Err(error) => {
                        assert!(!error.message.is_empty(), "{wrapped}");
                        *rejected += 1;
                    }
                }
            }
        }
        if depth == 0 {
            return;
        }
        for index in 0..ALPHABET.len() {
            indices.push(index);
            visit(indices, depth - 1, accepted, rejected);
            indices.pop();
        }
    }
    visit(&mut indices, 5, &mut accepted, &mut rejected);
    assert!(accepted > 0 && rejected > accepted);
}
