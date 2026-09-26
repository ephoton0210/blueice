// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, compile_module, compile_module_with_limit, compile_module_with_limits,
    compile_with_limits, parse, parse_module, Bytecode, ClassElement, CompileError, CompileLimits,
    Expr, Opcode, Program, RuntimeError, Stmt, Value, Vm, VmConfig,
};

fn largest_compile_path(code: &Bytecode) -> usize {
    code.bytes().len()
        + code
            .child_code_units()
            .map(largest_compile_path)
            .max()
            .unwrap_or(0)
}

#[test]
fn module_top_level_using_declarations_compile_with_disposal() {
    for source in ["using resource = null;", "await using resource = null;"] {
        let module = parse_module(source).unwrap();
        let code = compile_module(&module).unwrap();
        let opcodes: Vec<_> = code
            .instructions()
            .map(|instruction| instruction.opcode)
            .collect();
        let disposal = if source.starts_with("await") {
            Opcode::DrainAsyncDisposables
        } else {
            Opcode::DisposeResources
        };
        assert!(opcodes.contains(&disposal), "{source}: {opcodes:?}");
    }
}

#[test]
fn module_byte_limit_rejects_every_partial_program() {
    for source in [
        "export function f() { return 1; }",
        "using resource = null;",
    ] {
        let module = parse_module(source).unwrap();
        let full = compile_module(&module).unwrap();
        let upper = u32::try_from(largest_compile_path(&full)).unwrap();
        let mut first_success = None;
        for limit in 0..=upper {
            match compile_module_with_limit(&module, limit) {
                Ok(code) => {
                    first_success.get_or_insert(limit);
                    assert_eq!(code.bytes(), full.bytes(), "{source} at {limit} bytes");
                }
                Err(CompileError::ProgramTooLarge) => {
                    assert!(first_success.is_none(), "{source} at {limit} bytes");
                }
                Err(error) => panic!("{source} at {limit} bytes: {error:?}"),
            }
        }
        assert!(first_success.is_some(), "{source}");
    }
}

#[test]
fn metadata_limits_reject_each_table_before_an_index_overflows() {
    let limits = CompileLimits {
        max_metadata_entries: 1,
        ..CompileLimits::default()
    };
    for source in ["let first, second;", "1; 2;", "{}", "1; missing++;"] {
        assert!(
            matches!(
                compile_with_limits(&parse(source).unwrap(), limits),
                Err(CompileError::ProgramTooLarge)
            ),
            "{source}"
        );
    }
    assert!(compile_with_limits(&parse("1;").unwrap(), limits).is_ok());
    let no_metadata = CompileLimits {
        max_metadata_entries: 0,
        ..CompileLimits::default()
    };
    assert!(matches!(
        compile_with_limits(&parse("missing++;").unwrap(), no_metadata),
        Err(CompileError::ProgramTooLarge)
    ));
    assert!(matches!(
        compile_module_with_limits(&parse_module("let first, second;").unwrap(), limits),
        Err(CompileError::ProgramTooLarge)
    ));
}

#[test]
fn eval_limits_cover_visible_bindings_and_the_final_halt() {
    let outer = compile(&parse("eval('');").unwrap()).unwrap();
    let mut vm = Vm::new(VmConfig {
        eval_compile_limits: CompileLimits {
            max_bytecode_bytes: 5,
            ..CompileLimits::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    assert!(matches!(
        vm.execute(&outer),
        Err(RuntimeError::SyntaxError(_))
    ));

    let mut vm = Vm::new(VmConfig {
        eval_compile_limits: CompileLimits {
            max_bytecode_bytes: 6,
            ..CompileLimits::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    assert_eq!(vm.execute(&outer), Ok(Value::Undefined));

    let outer = compile(&parse("let first = 1, second = 2; eval('1');").unwrap()).unwrap();
    let mut vm = Vm::new(VmConfig {
        eval_compile_limits: CompileLimits {
            max_metadata_entries: 1,
            ..CompileLimits::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    assert!(matches!(
        vm.execute(&outer),
        Err(RuntimeError::SyntaxError(_))
    ));
}

#[test]
fn compiler_rejects_an_undeclared_private_name_in_an_external_ast() {
    let program = Program {
        body: vec![Stmt::Expr(Expr::PrivateIn {
            name: "missing".into(),
            object: Box::new(Expr::Number(0.0)),
        })],
    };
    assert!(matches!(
        compile(&program),
        Err(CompileError::InvalidSyntax(
            "private name is not declared in an enclosing class"
        ))
    ));
}

#[test]
fn annex_b_block_function_does_not_replace_a_root_lexical_binding() {
    for source in [
        "let f = 7; { function f() {} } f;",
        "eval('let f = 7; { function f() {} } f;');",
    ] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert_eq!(Vm::default().execute(&code), Ok(Value::Number(7.0)));
    }
}

#[test]
fn direct_eval_resolves_the_innermost_private_name() {
    let source = "class Outer { #value = 1; run() { \
        class Inner { #value = 2; read() { return eval('this.#value'); } } \
        return new Inner().read(); } } new Outer().run();";
    let code = compile(&parse(source).unwrap()).unwrap();
    assert_eq!(Vm::default().execute(&code), Ok(Value::Number(2.0)));

    let mut body = "return eval('this.#value');".to_owned();
    for depth in (1..=12).rev() {
        body = format!(
            "class C{depth} {{ #value = {depth}; read() {{ {body} }} }} \
             return new C{depth}().read();"
        );
    }
    let source = format!("(function() {{ {body} }})();");
    let code = compile(&parse(&source).unwrap()).unwrap();
    assert_eq!(Vm::default().execute(&code), Ok(Value::Number(12.0)));
}

#[test]
fn top_level_using_is_rejected_in_scripts_and_direct_eval() {
    let script = parse("using resource = null;").unwrap();
    assert!(matches!(
        compile(&script),
        Err(CompileError::InvalidSyntax(
            "a using declaration is not allowed directly at the top level of a Script"
        ))
    ));

    let code = compile(&parse("eval('using resource = null;');").unwrap()).unwrap();
    assert!(matches!(
        Vm::default().execute(&code),
        Err(RuntimeError::SyntaxError(_))
    ));

    let code = compile(&parse("eval('{ using resource; }');").unwrap()).unwrap();
    assert!(matches!(
        Vm::default().execute(&code),
        Err(RuntimeError::SyntaxError(_))
    ));
}

#[test]
fn uncommon_declarations_and_inferred_names_compile_and_execute() {
    for (source, expected) in [
        (
            "switch (0) { case 0: class C {} C.name; }",
            Value::String("C".into()),
        ),
        (
            "if (false) {} else function f() { return 3; } f();",
            Value::Number(3.0),
        ),
        (
            "try { function f() { return 9; } } finally {} f();",
            Value::Number(9.0),
        ),
        (
            "try { throw 0; } catch (error) { function f() { return 8; } } f();",
            Value::Number(8.0),
        ),
        (
            "try {} finally { function f() { return 7; } } f();",
            Value::Number(7.0),
        ),
        (
            "let f = (function() {}); f.name;",
            Value::String("f".into()),
        ),
        (
            "class C { static accessor ['value'] = 3; } C.value;",
            Value::Number(3.0),
        ),
        (
            "let count = 0; class C { [++count] = 4; static [++count] = 5; } \
             let c = new C(); count === 2 && c[1] === 4 && C[2] === 5;",
            Value::Bool(true),
        ),
    ] {
        let code = compile(&parse(source).unwrap()).unwrap();
        assert_eq!(Vm::default().execute(&code), Ok(expected), "{source}");
    }
}

#[test]
fn parenthesized_anonymous_class_field_initializer_gets_the_field_name() {
    let mut program = parse("class C { value = function() {}; } new C().value.name;").unwrap();
    let Stmt::ClassDecl(class) = &mut program.body[0] else {
        panic!("expected a class declaration");
    };
    let ClassElement::Field {
        initializer: Some(initializer),
        ..
    } = &mut class.elements[0]
    else {
        panic!("expected a class field initializer");
    };
    *initializer = Expr::Parenthesized(Box::new(initializer.clone()));
    let code = compile(&program).unwrap();
    assert_eq!(
        Vm::default().execute(&code),
        Ok(Value::String("value".into()))
    );
}

#[test]
fn strict_assignment_validation_walks_nested_statement_and_expression_shapes() {
    // Parse as a sloppy script first, then turn the AST into strict code. This
    // exercises compile's validation even for forms a strict parser rejects.
    for source in [
        "while (false) { eval = 1; }",
        "do { eval = 1; } while (false);",
        "with ({}) { eval = 1; }",
        "with ((eval = 1)) {}",
        "for (eval = 1; false;) {}",
        "for (eval in {}) {}",
        "for (var eval = 1 in {}) {}",
        "for (var x = (eval = 1) in {}) {}",
        "for (target(eval = 1) in {}) {}",
        "let {...eval} = {};",
        "({...eval} = {});",
        "let {[(eval = 1)]: value} = {};",
        "(eval) = 1;",
        "({...(eval = 1)});",
        "f?.(eval = 1);",
    ] {
        let mut program = parse(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        program
            .body
            .insert(0, Stmt::Expr(Expr::String("use strict".into())));
        assert!(
            matches!(
                compile(&program),
                Err(CompileError::InvalidSyntax(
                    "strict code cannot assign to eval or arguments"
                ))
            ),
            "{source}"
        );
    }
}

#[test]
fn strict_annex_b_for_in_without_restricted_names_fails_for_the_right_reason() {
    let mut program = parse("for (var x = 1 in {}) {}").unwrap();
    program
        .body
        .insert(0, Stmt::Expr(Expr::String("use strict".into())));
    assert!(matches!(
        compile(&program),
        Err(CompileError::InvalidSyntax(message))
            if message != "strict code cannot assign to eval or arguments"
    ));
}
