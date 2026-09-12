// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compiler/VM acceptance tests through the three real public stages.
use blueice_bluejs::{
    compile, parse, CompileError, HeapConfig, Opcode, RuntimeError, Value, Vm, VmConfig,
};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    let code = compile(&parse(source).unwrap()).unwrap();
    Vm::default().execute(&code)
}

#[test]
fn compilation_emits_a_stack_program_with_fixed_width_operands() {
    let code = compile(&parse("1 + 2 * 3;").unwrap()).unwrap();
    let instructions: Vec<_> = code.instructions().collect();
    assert_eq!(
        instructions.iter().map(|i| i.opcode).collect::<Vec<_>>(),
        [
            Opcode::EnterScope,
            Opcode::Constant,
            Opcode::Constant,
            Opcode::Constant,
            Opcode::Multiply,
            Opcode::Add,
            Opcode::SetCompletion,
            Opcode::Halt
        ]
    );
    let mut offset = 0;
    for instruction in instructions {
        assert_eq!(instruction.offset, offset);
        assert_eq!(code.bytes()[offset], instruction.opcode as u8);
        if let Some(operand) = instruction.operand {
            assert_eq!(
                &code.bytes()[offset + 1..offset + 5],
                &operand.to_le_bytes()
            );
        }
        offset += instruction.opcode.width();
    }
    assert_eq!(offset, code.bytes().len());
    assert_eq!(
        code.constants(),
        &[Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]
    );
    assert_eq!(
        Opcode::GetProperty.flags(),
        blueice_bluejs::MAY_USE_INLINE_CACHE
    );
    assert_eq!(Opcode::Add.flags(), 0);
    assert_eq!(Vm::default().execute(&code).unwrap(), Value::Number(7.0));
}

#[test]
fn optional_chains_short_circuit_remaining_suffixes_and_preserve_method_receivers() {
    assert_eq!(
        evaluate(
            "let calls=0; let absent=null; let result=absent?.[calls++].value(calls++); result===undefined&&calls===0",
        ),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        evaluate(
            "let object={inner:{value:42,method(){return this.value}}}; object?.inner.method()===42",
        ),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        evaluate("let calls=0; let fn=null; fn?.(calls++); calls===0",),
        Ok(Value::Bool(true))
    );
    assert!(matches!(
        evaluate("let absent=null; (absent?.value).next"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn numeric_strings_use_js_grammar_whitespace_and_single_rounding() {
    for (input, expected) in [
        (".5", 0.5),
        ("1.", 1.0),
        ("-0", -0.0),
        ("+Infinity", f64::INFINITY),
        ("-Infinity", f64::NEG_INFINITY),
        ("1e999", f64::INFINITY),
        ("1e-999", 0.0),
        ("0x00000000", 0.0),
        ("0x20000000000001", 9007199254740992.0),
        ("0x20000000000003", 9007199254740996.0),
        ("0x40000000000003", 18014398509481988.0),
        ("0X10", 16.0),
        ("0B11", 3.0),
        ("0O7", 7.0),
        ("\u{feff}\u{a0}7\u{2028}", 7.0),
    ] {
        let result = evaluate(&format!("+'{input}'")).unwrap();
        let Value::Number(n) = result else {
            panic!("expected number")
        };
        assert_eq!(n.to_bits(), expected.to_bits(), "{input}");
    }
    for input in [
        ".", "+", "1e", "1e+", "1.2x", "１", "0x", "0b2", "-0x10", "+0o2", "inf", "\u{85}",
    ] {
        assert!(
            matches!(evaluate(&format!("+'{input}'")).unwrap(), Value::Number(n) if n.is_nan()),
            "{input}"
        );
    }
    let huge = format!("+'0x1{}'", "0".repeat(256));
    assert_eq!(evaluate(&huge).unwrap(), Value::Number(f64::INFINITY));
}

#[test]
fn primitive_string_formatting_and_utf16_comparison_cover_boundaries() {
    for (source, expected) in [
        ("''+undefined+null+true+false", "undefinednulltruefalse"),
        ("''+-0", "0"),
        ("''+NaN+Infinity+-Infinity", "NaNInfinity-Infinity"),
        ("''+1e-7", "1e-7"),
        ("''+1e-6", "0.000001"),
        ("''+1e20", "100000000000000000000"),
        ("''+1e21", "1e+21"),
        ("''+1000000000000000128", "1000000000000000100"),
        ("''+1.1142895159317463e15", "1114289515931746.2"),
        ("''+-1.1142895159317463e15", "-1114289515931746.2"),
        ("typeof true", "boolean"),
        ("typeof 1", "number"),
        ("typeof 'x'", "string"),
        ("typeof {}", "object"),
    ] {
        assert_eq!(
            evaluate(source).unwrap(),
            Value::String(expected.into()),
            "{source}"
        );
    }
    assert_eq!(
        evaluate("'😀' < '￿' && '😀' > 'a' && !!{}").unwrap(),
        Value::Bool(true)
    );
    for source in ["+'bad' <= 1", "NaN > 1", "NaN === 1"] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(false));
    }
}

#[test]
fn every_compound_store_and_prototype_literal_form_executes() {
    for (source, expected) in [
        ("let x=8; x-=2; x*=3; x/=2; x%=4; x", 1.0),
        ("let o={x:8}; o.x-=2; o.x*=3; o.x/=2; o.x%=4; o.x", 1.0),
        ("for(var i=0;i<2;i++){} i", 2.0),
        ("let __proto__=3; ({__proto__}).__proto__", 3.0),
        ("let p={x:3}; let o={__proto__:p}; o.x++; o.x*10+p.x", 43.0),
    ] {
        assert_eq!(
            evaluate(source).unwrap(),
            Value::Number(expected),
            "{source}"
        );
    }
    assert_eq!(
        evaluate("({__proto__:3}).missing").unwrap(),
        Value::Undefined
    );
    assert_eq!(
        evaluate("let o={x:4}; let old=o; o.x=(o={x:8}); old.x===o").unwrap(),
        Value::Bool(true)
    );
    assert_eq!(evaluate("'x'.length").unwrap(), Value::Number(1.0));
    for source in [
        "!(({}) < 1)",
        "''+{} === '[object Object]'",
        "`${{}}` === '[object Object]'",
        "({})[{}] === undefined",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn heap_errors_release_temporary_roots_and_previous_results_even_on_failure() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 1024,
            max_heap_bytes: 2048,
        },
        ..VmConfig::default()
    })
    .unwrap();
    let baseline = vm.heap().stats().managed_bytes;
    for source in [
        "let o={}; while(true){o={next:o};}",
        "let o={}; let s='xxxxxxxx'; for(let i=0;i<9;i++){s+=s;} o.x=s;",
    ] {
        let result = vm
            .execute(&compile(&parse("({keep:{}})").unwrap()).unwrap())
            .unwrap();
        let Value::Object(old) = result else {
            panic!("expected object")
        };
        let error = vm
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap_err();
        assert_eq!(
            error,
            RuntimeError::Heap(blueice_bluejs::HeapError::HeapLimitExceeded { limit: 2048 })
        );
        assert!(std::error::Error::source(&error).is_some());
        assert!(!error.to_string().is_empty());
        assert_eq!(
            vm.heap().stats().managed_bytes,
            baseline,
            "failed safepoint leaked a root"
        );
        assert!(!vm.heap().contains(old));
        assert_eq!(
            vm.execute(&compile(&parse("42").unwrap()).unwrap())
                .unwrap(),
            Value::Number(42.0)
        );
    }
    // Completion values, not just locals and operand-stack values, root
    // objects while later declarations allocate unrelated garbage.
    let Value::Object(object) = vm
        .execute(&compile(&parse("({keep:{x:7}}); let garbage={};").unwrap()).unwrap())
        .unwrap()
    else {
        panic!("expected completion")
    };
    assert!(matches!(
        vm.heap().get(object, "keep").unwrap(),
        Value::Object(_)
    ));
}

#[test]
fn configuration_boundaries_and_error_messages_are_observable() {
    assert!(matches!(
        Vm::new(VmConfig {
            heap: HeapConfig {
                nursery_capacity: 0,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        }),
        Err(blueice_bluejs::HeapError::InvalidConfig)
    ));
    assert!(matches!(
        Vm::new(VmConfig {
            heap: HeapConfig {
                major_threshold_bytes: 1,
                max_heap_bytes: 1,
                ..HeapConfig::default()
            },
            ..VmConfig::default()
        }),
        Err(blueice_bluejs::HeapError::InvalidConfig)
    ));
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 0,
        ..VmConfig::default()
    })
    .unwrap();
    assert_eq!(
        vm.execute(&compile(&parse("").unwrap()).unwrap()),
        Err(RuntimeError::InstructionLimit)
    );
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 3,
        ..VmConfig::default()
    })
    .unwrap();
    for source in ["'four'", "typeof 0", "`${undefined}`", "''+1000"] {
        assert_eq!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap()),
            Err(RuntimeError::StringLimit { limit: 3 }),
            "{source}"
        );
    }
    assert_eq!(
        vm.execute(&compile(&parse("'冰'").unwrap()).unwrap())
            .unwrap(),
        Value::String("冰".into())
    );
    for source in ["missing", "null.x", "String(Symbol())+Symbol()"] {
        let error = evaluate(source).unwrap_err();
        assert!(!error.to_string().is_empty());
        assert!(std::error::Error::source(&error).is_none());
    }
    assert!(RuntimeError::InstructionLimit
        .to_string()
        .contains("budget"));
    assert!(RuntimeError::StringLimit { limit: 3 }
        .to_string()
        .contains('3'));
    for source in ["let x;let x", "break", "let [x]"] {
        assert!(!compile(&parse(source).unwrap())
            .err()
            .unwrap()
            .to_string()
            .is_empty());
    }
}

#[test]
fn hand_built_invalid_ast_returns_compile_errors_and_code_is_reusable() {
    use blueice_bluejs::{Expr, Program, Stmt};
    for expr in [
        Expr::Template {
            quasis: vec![],
            expressions: vec![],
        },
        Expr::Member {
            object: Box::new(Expr::Null),
            property: Box::new(Expr::Null),
            computed: false,
        },
        Expr::Assign {
            op: blueice_bluejs::AssignOp::Assign,
            target: Box::new(Expr::Null),
            value: Box::new(Expr::Null),
        },
    ] {
        assert!(matches!(
            compile(&Program {
                body: vec![Stmt::Expr(expr)]
            }),
            Err(CompileError::InvalidSyntax(_))
        ));
    }
    let code = compile(&parse("let o={x:1}; o.x++; o").unwrap()).unwrap();
    let mut a = Vm::default();
    let mut b = Vm::default();
    let first = a.execute(&code).unwrap();
    let second = b.execute(&code).unwrap();
    assert_ne!(first, second);
    let Value::Object(third) = a.execute(&code).unwrap() else {
        panic!("expected object")
    };
    assert_ne!(first, Value::Object(third));
    assert_eq!(a.heap().get(third, "x").unwrap(), Value::Number(2.0));
    let loops =
        compile(&parse("for(let i=0;i<2;i++){if(i)continue;else break;}").unwrap()).unwrap();
    let loop_offsets: std::collections::HashSet<_> =
        loops.instructions().map(|i| i.offset).collect();
    for instruction in loops.instructions() {
        if matches!(
            instruction.opcode,
            Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::JumpIfNotNullish
        ) {
            assert!(loop_offsets.contains(&(instruction.operand.unwrap() as usize)));
        }
    }
}

#[test]
fn arithmetic_and_primitive_coercions_execute_with_js_behavior() {
    for (source, value) in [
        ("8 - 3 * 2", Value::Number(2.0)),
        ("8 / 2 + 7 % 4", Value::Number(7.0)),
        ("'x' + 3", Value::String("x3".into())),
        ("true + null", Value::Number(1.0)),
        ("+' 2.5e1 ' + -'3'", Value::Number(22.0)),
        ("+''", Value::Number(0.0)),
        ("+'0x10' + +'0b11' + +'0o7'", Value::Number(26.0)),
        ("typeof missing", Value::String("undefined".into())),
        ("typeof null", Value::String("object".into())),
        (
            "!0 && !'' && !null && !undefined && !NaN",
            Value::Bool(true),
        ),
        ("'2' === 2", Value::Bool(false)),
        ("'2' !== 2", Value::Bool(true)),
        ("'12' < '2'", Value::Bool(true)),
        ("'12' < 2", Value::Bool(false)),
        ("3 <= 3 && 4 > 3 && 4 >= 4", Value::Bool(true)),
        ("NaN === NaN", Value::Bool(false)),
        ("NaN < 1 || NaN >= 1", Value::Bool(false)),
        (
            "`a${1 + 2}:${false}:${null}`",
            Value::String("a3:false:null".into()),
        ),
    ] {
        assert_eq!(evaluate(source).unwrap(), value, "{source}");
    }
    assert!(matches!(evaluate("undefined + 1").unwrap(), Value::Number(n) if n.is_nan()));
    assert!(matches!(evaluate("1 / -0").unwrap(), Value::Number(n) if n == f64::NEG_INFINITY));
}

#[test]
fn short_circuiting_and_conditionals_skip_unevaluated_branches() {
    for (source, value) in [
        ("false && missing", Value::Bool(false)),
        ("7 || missing", Value::Number(7.0)),
        ("0 ?? missing", Value::Number(0.0)),
        ("null ?? 8", Value::Number(8.0)),
        ("undefined ?? 9", Value::Number(9.0)),
        ("true ? 1 : missing", Value::Number(1.0)),
        ("false ? missing : 2", Value::Number(2.0)),
        (
            "let x = 0; false && x++; true || x++; 1 ?? x++; x",
            Value::Number(0.0),
        ),
    ] {
        assert_eq!(evaluate(source).unwrap(), value, "{source}");
    }
}

#[test]
fn variables_are_hoisted_or_block_scoped_and_const_writes_fail() {
    for (source, value) in [
        ("x; var x;", Value::Undefined),
        ("if (false) { var x = 4; } x", Value::Undefined),
        ("var x=1; var x; x", Value::Number(1.0)),
        ("let x=1; { let x=2; x+=5; } x", Value::Number(1.0)),
        ("let x=1; { var y=2; x+=y; } x+y", Value::Number(5.0)),
        ("let x; x=3; x", Value::Number(3.0)),
        ("const o={}; o.x=4; o.x", Value::Number(4.0)),
    ] {
        assert_eq!(evaluate(source).unwrap(), value, "{source}");
    }
    assert!(matches!(
        evaluate("const x=1; x=2"),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        evaluate("let x=1; { x; let x=2; }"),
        Err(RuntimeError::ReferenceError(_))
    ));
    assert!(matches!(
        evaluate("{let x=1;} x"),
        Err(RuntimeError::ReferenceError(_))
    ));
}

#[test]
fn loops_unwind_nested_scopes_on_break_and_continue() {
    for (source, result) in [
        ("let s=0; for(let i=0;i<6;i++){ if(i===2)continue; if(i===5)break; {let x=i; s+=x;} } s", 8.0),
        ("let i=0; let s=0; while(i<5){i++; {let v=i; if(i===2)continue; s+=v;}} s", 13.0),
        ("let i=0; do {i++; if(i<3)continue; break;} while(true); i", 3.0),
        ("let n=0; for(let i=0;i<3;i++){for(let j=0;j<4;j++){if(j===2)break;n++;}} n", 6.0),
        ("let i=0; for(;;){i++; if(i===3)break;} i", 3.0),
        ("var i=0; for(i=0;i<2;i++) {} i", 2.0),
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Number(result), "{source}");
    }
    assert!(matches!(
        evaluate("for(let i=0;i<1;i++){} i"),
        Err(RuntimeError::ReferenceError(_))
    ));
}

#[test]
fn statement_completion_distinguishes_empty_blocks_and_control_statements() {
    for (source, result) in [
        ("", Value::Undefined),
        ("1; ; {} var x;", Value::Number(1.0)),
        ("1; if(false) 2;", Value::Undefined),
        ("1; if(true) {}", Value::Undefined),
        ("1; while(false) 2;", Value::Undefined),
        ("let i=0; while(i<3) {i++; i*2;}", Value::Number(6.0)),
        ("let i=0; do { i++; } while(false);", Value::Number(0.0)),
    ] {
        assert_eq!(evaluate(source).unwrap(), result, "{source}");
    }
}

#[test]
fn object_operations_preserve_identity_and_evaluation_order() {
    for (source, result) in [
        ("let a={x:2}; let b=a; b.x+=3; a.x", Value::Number(5.0)),
        ("let a={}; a===a && a!=={}", Value::Bool(true)),
        (
            "let k='x'; let o={ [k]:3, k }; o.x + o.k",
            Value::String("3x".into()),
        ),
        (
            "let i=0; let o={0:3}; let a=o[i++]++; a*100+o[0]*10+i",
            Value::Number(341.0),
        ),
        (
            "let i=0; let o={0:3}; o[i++] += (i=5); o[0]*10+i",
            Value::Number(85.0),
        ),
        (
            "let o={x:'2'}; let a=++o.x; let b=o.x--; a*100+b*10+o.x",
            Value::Number(332.0),
        ),
        (
            "let x='2'; let a=x++; let b=--x; a*100+b*10+x",
            Value::Number(222.0),
        ),
        ("let p={x:3}; let o={__proto__:p}; o.x", Value::Number(3.0)),
        ("let o={['__proto__']:7}; o.__proto__", Value::Number(7.0)),
        ("({__proto__:null}).missing", Value::Undefined),
    ] {
        assert_eq!(evaluate(source).unwrap(), result, "{source}");
    }
}

#[test]
fn function_frames_create_mapped_and_unmapped_arguments_objects() {
    for source in [
        "function mapped(a,b){let copy=arguments;a=3;copy[1]=4;return copy.length===2&&copy[0]===3&&b===4&&copy.callee===mapped;}mapped(1,2)",
        "function disconnected(a){delete arguments[0];a=7;return arguments[0]===undefined;}disconnected(1)",
        "function unmapped(a=1){a=2;return arguments[0]===1;}unmapped(1)",
        "function outer(a){return (()=>arguments[0])()===7;}outer(7)",
        "function* generated(a){return arguments.callee===generated&&arguments[0]===a;}generated(3).next().value",
        "function prototype(){return arguments.constructor.prototype===Object.prototype;}prototype()",
        "(function(){'use strict';try{arguments.callee;return false;}catch(error){return error instanceof TypeError;}})()",
    ] {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn unsupported_syntax_and_invalid_bindings_fail_before_execution() {
    let source = "let undefined=1";
    let code = compile(&parse(source).unwrap()).expect("the parser accepts the lexical name");
    assert!(
        matches!(
            Vm::default().execute_script(&code),
            Err(RuntimeError::SyntaxError(_))
        ),
        "{source}"
    );
    for source in ["for(x of y){}", "for(x in y){}"] {
        assert!(compile(&parse(source).unwrap()).is_ok(), "{source}");
    }
    assert_eq!(evaluate("for(x of [1,2]){};x").unwrap(), Value::Number(2.0));
    assert_eq!(
        evaluate("for(x in {key:1}){};x").unwrap(),
        Value::String("key".into())
    );
    for source in [
        "'use strict';for(x of [1]){}",
        "'use strict';for(x in {key:1}){}",
    ] {
        let code = compile(&parse(source).unwrap()).expect("strict loop should compile");
        assert!(
            matches!(
                Vm::default().execute(&code),
                Err(RuntimeError::ReferenceError(_))
            ),
            "{source}"
        );
    }
    assert_eq!(evaluate("x=1; globalThis.x").unwrap(), Value::Number(1.0));
    let strict_unbound = compile(&parse("'use strict'; x=1").unwrap())
        .expect("strict unbound assignment should compile");
    assert!(matches!(
        Vm::default().execute(&strict_unbound),
        Err(RuntimeError::ReferenceError(_))
    ));
    for source in ["x == 1", "x != 1", "x in y"] {
        assert!(compile(&parse(source).unwrap()).is_ok(), "{source}");
    }
    for source in [
        "return 1",
        "break",
        "continue",
        "let x; let x;",
        "let x; var x;",
        "{let x; {var x;}}",
        "const x;",
        "if(true) let x=1;",
        "({__proto__:null,__proto__:null})",
    ] {
        assert!(compile(&parse(source).unwrap()).is_err(), "{source}");
    }
}

#[test]
fn runtime_errors_and_limits_leave_the_vm_reusable() {
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 1000,
        max_string_bytes: 128,
        ..VmConfig::default()
    })
    .unwrap();
    for source in [
        "missing",
        "const x=1; x++",
        "null.x",
        "String({toString:1,valueOf:2})",
        "while(true) {}",
        "let s='x'; while(true){s+=s;}",
    ] {
        assert!(
            vm.execute(&compile(&parse(source).unwrap()).unwrap())
                .is_err(),
            "{source}"
        );
        assert_eq!(
            vm.execute(&compile(&parse("40+2").unwrap()).unwrap())
                .unwrap(),
            Value::Number(42.0)
        );
    }
    assert_eq!(
        vm.execute(&compile(&parse("for(;;){}").unwrap()).unwrap()),
        Err(RuntimeError::InstructionLimit)
    );
    assert_eq!(
        vm.execute(&compile(&parse("let s='x';while(true){s+=s;}").unwrap()).unwrap()),
        Err(RuntimeError::StringLimit { limit: 128 })
    );
}

#[test]
fn objects_on_the_stack_and_in_bindings_survive_gc_and_results_live_until_the_next_run() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 1024,
            max_heap_bytes: 8192,
        },
        ..VmConfig::default()
    })
    .unwrap();
    let code = compile(&parse("let keep={x:4}; let o={a:{b:{c:7}}, d:{x:8}}; for(let i=0;i<40;i++){let garbage={n:i};} o.keep=keep; o").unwrap()).unwrap();
    let Value::Object(object) = vm.execute(&code).unwrap() else {
        panic!("expected object")
    };
    let Value::Object(keep) = vm.heap().get(object, "keep").unwrap() else {
        panic!("expected child")
    };
    assert_eq!(vm.heap().get(keep, "x").unwrap(), Value::Number(4.0));
    let Value::Object(a) = vm.heap().get(object, "a").unwrap() else {
        panic!("expected child")
    };
    let Value::Object(b) = vm.heap().get(a, "b").unwrap() else {
        panic!("expected child")
    };
    assert_eq!(vm.heap().get(b, "c").unwrap(), Value::Number(7.0));
    assert!(vm.heap().stats().minor_collections > 1);
    assert_eq!(
        vm.execute(&compile(&parse("1").unwrap()).unwrap()).unwrap(),
        Value::Number(1.0)
    );
    assert!(!vm.heap().contains(object) && !vm.heap().contains(keep));
}
