// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JavaScript call nesting is bounded by the native stack that is actually
//! left, not by a frame count.
//!
//! Every interpreted call recurses through Rust's own call stack, so running
//! out of native stack is a process abort rather than a catchable error. These
//! tests drive the public `Vm` interface on threads of a known, exact stack
//! size to prove both halves of the contract: legitimate deep recursion (a
//! 100-deep constructor chain, hundreds of nested calls) succeeds on a normal
//! process stack, and runaway recursion still ends in a catchable `RangeError`
//! -- never a stack overflow -- on stacks from tiny to huge.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

/// The stack a normal process's main thread receives on macOS and Linux.
const NORMAL_PROCESS_STACK: usize = 8 * MIB;

/// Run `source` in a fresh VM on a thread with exactly `stack_size` bytes of
/// stack, so the result does not depend on the test runner's worker size.
fn run_on_stack(stack_size: usize, source: &'static str) -> Result<Value, RuntimeError> {
    std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(move || {
            let mut vm = Vm::new(VmConfig {
                instruction_budget: 50_000_000,
                ..VmConfig::default()
            })
            .unwrap();
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
        })
        .unwrap()
        .join()
        .unwrap()
}

fn is_range_error(result: &Result<Value, RuntimeError>) -> bool {
    matches!(result, Err(RuntimeError::RangeError(_)))
}

/// `staging/sm/class/superPropChains.js`: a class chain 100 constructors long,
/// each calling `super()`, then a `super.testChain()` walk up it.
const SUPER_PROP_CHAINS: &str = r#"
class base {
    constructor() { }
    testChain() { this.baseCalled = true; }
}
base.prototype.x = "yeehaw";
let chain = class extends base { constructor() { super(); } };
const CHAIN_LENGTH = 100;
for (let i = 0; i < CHAIN_LENGTH; i++)
    chain = class extends chain { constructor() { super(); } };
let inst = new chain();
inst.testChain();
inst.baseCalled === true && inst.x === "yeehaw"
"#;

#[test]
fn a_hundred_deep_constructor_chain_runs_on_a_normal_process_stack() {
    assert_eq!(
        run_on_stack(NORMAL_PROCESS_STACK, SUPER_PROP_CHAINS),
        Ok(Value::Bool(true))
    );
}

#[test]
fn nested_calls_go_far_beyond_the_old_thirty_two_frame_bound() {
    assert_eq!(
        run_on_stack(
            NORMAL_PROCESS_STACK,
            "function depth(n){return n===0?0:1+depth(n-1);} depth(200)"
        ),
        Ok(Value::Number(200.0))
    );
}

#[test]
fn runaway_recursion_is_a_range_error_at_every_stack_size() {
    // From a stack barely larger than the safety margin to one far larger than
    // any real process gets. A missing or mis-sized guard aborts the whole
    // test process with a stack overflow instead of failing one assertion.
    for stack_size in [512 * KIB, MIB, 2 * MIB, NORMAL_PROCESS_STACK, 64 * MIB] {
        let result = run_on_stack(stack_size, "function r(){return 1+r();} r()");
        assert!(
            is_range_error(&result),
            "{stack_size}-byte stack: {result:?}"
        );
    }
}

#[test]
fn a_stack_smaller_than_the_safety_margin_refuses_calls_instead_of_overflowing() {
    // 128 KiB cannot hold a single interpreted call plus the margin; the call
    // must fail cleanly. (The top-level script itself needs no call frame.)
    let result = run_on_stack(128 * KIB, "function f(){return 1;} f()");
    assert!(is_range_error(&result), "{result:?}");
}

/// Recursion that never terminates, through every host path that re-enters the
/// interpreter: each of these must end in a `RangeError`, because the margin
/// left below the guard has to cover the native frames of whichever built-in
/// sits between two interpreted calls.
const RUNAWAY_PATTERNS: &[(&str, &str)] = &[
    ("plain call", "function f(){return 1+f();} f()"),
    ("arrow", "const f=()=>1+f(); f()"),
    (
        "Function.prototype.call",
        "function f(){return 1+f.call(null);} f()",
    ),
    (
        "Function.prototype.apply",
        "function f(){return 1+f.apply(null,[]);} f()",
    ),
    (
        "Reflect.apply",
        "function f(){return 1+Reflect.apply(f,null,[]);} f()",
    ),
    (
        "bound function",
        "function f(){return 1+f.bind(null)();} f()",
    ),
    (
        "spread call",
        "function f(...a){return 1+f(...a);} f(1,2,3)",
    ),
    ("getter", "var o={get x(){return 1+o.x;}}; o.x"),
    (
        "toString coercion",
        "var o={toString(){return ''+o;}}; ''+o",
    ),
    ("valueOf coercion", "var o={valueOf(){return +o;}}; +o"),
    (
        "Symbol.hasInstance",
        "var C={[Symbol.hasInstance](v){return v instanceof C;}}; 1 instanceof C",
    ),
    (
        "Array.prototype.map",
        "function f(n){return [n].map(f)[0];} f(1)",
    ),
    (
        "Array.prototype.sort",
        "function f(){[2,1].sort(function(){f();return 0;});} f()",
    ),
    (
        "String.prototype.replace",
        "function f(){return 'a'.replace('a',function(){return f();});} f()",
    ),
    (
        "Array.from",
        "function f(){return Array.from([1],function(){return f();});} f()",
    ),
    (
        "Proxy get trap",
        "var p=new Proxy({},{get(t,k){return 1+p.x;}}); p.x",
    ),
    (
        "Proxy apply trap",
        "var p=new Proxy(function(){},{apply(){return 1+p();}}); p()",
    ),
    ("direct eval", "function f(){return 1+eval('f()');} f()"),
    ("constructor", "class A{constructor(){new A();}} new A()"),
    (
        "super() chain",
        "class A{constructor(){new B();}} class B extends A{constructor(){super();}} new B()",
    ),
    (
        "Reflect.construct",
        "function F(){Reflect.construct(F,[]);} new F()",
    ),
    (
        "generator delegation",
        "function* g(){yield* g();} [...g()]",
    ),
    ("tagged template", "function t(){return 1+t`x`;} t()"),
    ("default parameter", "function f(x=f()){return x;} f()"),
    (
        "try/finally",
        "function f(){try{return 1+f();}finally{}} f()",
    ),
];

#[test]
fn runaway_recursion_through_every_reentrant_host_path_ends_in_a_range_error() {
    // 2 MiB is Rust's default test-worker stack; 8 MiB is a normal process.
    for stack_size in [2 * MIB, NORMAL_PROCESS_STACK] {
        for (name, source) in RUNAWAY_PATTERNS {
            let result = run_on_stack(stack_size, source);
            assert!(
                is_range_error(&result),
                "{name} on a {stack_size}-byte stack: {result:?}"
            );
        }
    }
}

#[test]
fn a_caught_stack_range_error_leaves_the_engine_fully_usable() {
    // Each runaway recursion must unwind completely: the next one starts from
    // the same depth budget, and ordinary calls afterwards still work.
    assert_eq!(
        run_on_stack(
            NORMAL_PROCESS_STACK,
            r#"
            function r(){return 1+r();}
            var caught = 0;
            for (var i = 0; i < 3; i++) {
                try { r(); } catch (e) { if (e instanceof RangeError) caught++; }
            }
            function depth(n){return n===0?0:1+depth(n-1);}
            caught === 3 && depth(100) === 100
            "#
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn handlers_running_at_the_recursion_limit_can_still_call_functions() {
    // Every frame catches the error from the frame below it and calls another
    // function from its handler: near the limit those calls are refused again
    // and the error keeps unwinding, until a frame with room returns normally.
    assert_eq!(
        run_on_stack(
            NORMAL_PROCESS_STACK,
            r#"
            function one(){return 1;}
            function r(){ try { return r(); } catch (e) { return one(); } }
            r()
            "#
        ),
        Ok(Value::Number(1.0))
    );
}
