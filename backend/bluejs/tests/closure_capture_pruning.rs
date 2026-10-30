// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A closure is given the names its source mentions, not every name in scope where it is
//! made. Whatever a closure can still reach by name must behave exactly as before: reads,
//! writes, shadowing, the temporal dead zone, `eval` and `with`, and names used only by
//! functions nested deeper.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn run(source: &str) -> Value {
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    vm.execute(&compile(&parse(source).expect(source)).expect(source))
        .unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

fn number(source: &str) -> f64 {
    match run(source) {
        Value::Number(n) => n,
        other => panic!("{source}: {other:?}"),
    }
}

/// Each script wraps its own scope with unused names around the closure under test.
const NUMBERS: &[(&str, f64)] = &[
    // Plain reads and writes through a closure.
    ("(function(){ let a=1,b=2,c=3; return function(){ b = b + 10; return b; }; })()()", 12.0),
    ("(function(){ let a=1,b=2; const f=()=>a+b; a=5; return f(); })()", 7.0),
    // A name used only by a function nested two levels down.
    ("(function(){ let a=4,b=9; return function(){ return function(){ return a; }; }; })()()()", 4.0),
    // Shadowing: the inner declaration wins, the outer one is untouched.
    ("(function(){ let a=1; function f(){ let a=2; return a; } return f()*10 + a; })()", 21.0),
    // Parameters and defaults reading enclosing names.
    ("(function(){ let k=3; return function(x=k){ return x*2; }; })()()", 6.0),
    ("(function(){ let k=3; return function({y=k}){ return y; }; })()({})", 3.0),
    // Hoisting and function declarations referring to each other.
    ("(function(){ function even(n){ return n==0?1:odd(n-1); } function odd(n){ return n==0?0:even(n-1); } return even(10); })()", 1.0),
    // A recursive named function expression.
    ("(function(){ let unused=1; return (function fact(n){ return n<=1?1:n*fact(n-1); })(5); })()", 120.0),
    // Closures in a loop each keep their own binding.
    ("(function(){ let fs=[]; for (let i=0;i<3;i++) fs.push(()=>i); return fs[0]()+fs[1]()*10+fs[2]()*100; })()", 210.0),
    // Destructuring assignment to an enclosing name.
    ("(function(){ let a=0,b=0; (function(){ [a,b]=[3,4]; })(); return a*10+b; })()", 34.0),
    // typeof of a name in scope and of one that is not.
    ("(function(){ let a=1; return function(){ return (typeof a==='number'?1:0)+(typeof zzz==='undefined'?10:0); }; })()()", 11.0),
    // Template and tagged template.
    ("(function(){ let n=7; return (()=>`${n}`.length)(); })()", 1.0),
    // Class bodies: methods, static blocks, fields, computed keys, extends.
    ("(function(){ let n=5; class C { m(){ return n; } } return new C().m(); })()", 5.0),
    ("(function(){ let n=5; class C { x = n*2; } return new C().x; })()", 10.0),
    ("(function(){ let n=5; class C { static v; static { C.v = n+1; } } return C.v; })()", 6.0),
    ("(function(){ let k='m'; class C { [k](){ return 8; } } return new C().m(); })()", 8.0),
    ("(function(){ class B { f(){ return 1; } } let Base=B; return (function(){ class D extends Base { f(){ return super.f()+1; } } return new D().f(); })(); })()", 2.0),
    // Generators and async functions.
    ("(function(){ let n=3; function* g(){ yield n; yield n+1; } return [...g()].join('').length; })()", 2.0),
    ("(function(){ let r=0; (async function(){ let v=await 7; r=v; })(); return r; })()", 0.0),
    // The temporal dead zone is still observed through a closure.
    ("(function(){ const f=()=>t; let r=0; try { f(); } catch(e){ r=1; } let t=2; return r*10+f(); })()", 12.0),
    // `arguments` of an enclosing function through an arrow.
    ("(function(){ return (function(){ return (()=>arguments.length)(); })(1,2,3); })()", 3.0),
    // Direct eval sees every name of its function and the ones around it.
    ("(function(){ let a=2,b=3; return function(){ return eval('a*b'); }; })()()", 6.0),
    ("(function(){ let a=2,b=3; return function(){ return (()=>eval('a+b'))(); }; })()()", 5.0),
    // Indirect eval does not see locals, so it must not be given them.
    ("(function(){ var a=2; return (0,eval)('typeof a'); })()==='undefined'?1:0", 1.0),
    // `with` resolves through the object first, then through the enclosing names.
    ("(function(){ let a=1,b=2; return (function(){ with({a:10}){ return a+b; } })(); })()", 12.0),
    // Names that look like property names are just names.
    ("(function(){ let x=1; const o={x:2}; return function(){ return o.x + x; }; })()()", 3.0),
    // A capture count that grows with the scope: many unused names around a small closure.
    ("(function(){ let v0=0,v1=1,v2=2,v3=3,v4=4,v5=5,v6=6,v7=7,v8=8,v9=9; return function(){ return v3+v7; }; })()()", 10.0),
];

#[test]
fn closures_still_reach_every_name_they_use() {
    for (source, expected) in NUMBERS {
        assert_eq!(number(source), *expected, "{source}");
    }
}

#[test]
fn a_function_defined_in_a_script_sees_script_scope_lexicals() {
    assert_eq!(
        number("let a=2; const b=5; function f(){ return a*b; } f()"),
        10.0
    );
    assert_eq!(number("let a=2; function f(){ a=9; } f(); a"), 9.0);
}

#[test]
fn a_module_level_closure_sees_the_module_bindings() {
    let module = blueice_bluejs::parse_module(
        "let counter = 0; const step = 2; export function bump() { counter += step; return counter; }\n\
         export const result = (() => { bump(); return bump(); })();",
    )
    .unwrap();
    let program = blueice_bluejs::compile_module(&module).unwrap();
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    vm.execute_module(&program).expect("the module runs");
}

fn text(source: &str) -> String {
    match run(source) {
        Value::String(s) => s.to_utf8().expect("the script returns text"),
        other => panic!("{source}: {other:?}"),
    }
}

/// A closure made in code that a sloppy function's direct eval runs looks free names up by
/// name at run time, through the captures, so it must keep all of them: assigning to a named
/// function expression's own name (silently ignored in sloppy code) behaves exactly as it
/// does outside an eval.
#[test]
fn a_named_function_expression_in_a_direct_eval_keeps_its_self_binding_semantics() {
    for (body, expected) in [
        (
            "(function namedLambda(p) { namedLambda = function named() {}; return namedLambda.name; })()",
            "namedLambda",
        ),
        (
            "(function namedLambda(p) { namedLambda = function () {}; return namedLambda.name; })()",
            "namedLambda",
        ),
        (
            "(function namedLambda(p) { var a; a = namedLambda = function named() {}; return a.name; })()",
            "named",
        ),
    ] {
        let direct = format!("function t() {{ return {body}; }} t()");
        let in_eval = format!(
            "function t() {{ return eval(\"{}\"); }} t()",
            body.replace('\'', "\\'")
        );
        assert_eq!(text(&direct), expected, "{body}");
        assert_eq!(text(&in_eval), expected, "in eval: {body}");
    }
}

#[test]
fn closures_made_inside_a_with_still_see_the_names_around_it() {
    assert_eq!(
        number("(function(){ let a=1,b=2,unused=9; with ({c:3}) { return (function(){ return a+b+c; })(); } })()"),
        6.0
    );
    assert_eq!(
        number("(function(){ let a=5,unused=9; with ({}) { return (() => a)(); } })()"),
        5.0
    );
}
