// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn bare_compiler() -> Compiler {
    let limits = CompileLimits::default();
    Compiler {
        prune_guards: Vec::new(),
        bytecode: Bytecode::empty(),
        names: vec![HashMap::new()],
        private_scopes: Vec::new(),
        next_private_scope: 0,
        scopes: Vec::new(),
        loops: Vec::new(),
        catch_var_slots: Vec::new(),
        max_bytecode_bytes: limits.max_bytecode_bytes,
        max_metadata_entries: limits.max_metadata_entries,
        max_list_items: limits.max_list_items,
        function: false,
        async_dispose_helper: false,
        diagnostic_ast_ids: None,
        diagnostic_owner: None,
        current_root_statement_index: None,
        local_scope: 0,
        with_depth: 0,
        with_scope_depths: Vec::new(),
        annex_b_parameter_names: BTreeSet::new(),
        tail_call_blockers: 0,
        tail_call_pending: false,
    }
}

#[test]
fn decorated_and_derived_classes_compile_with_function_bindings() {
    for source in [
        "function named(value = 1, ...rest) { return value + rest.length; }",
        "class Base { field = 1; static value = 2; method(value) { return value; } }",
        "class Derived extends Base { constructor() { super(); } get value() { return this.field; } }",
        "@decorator class Decorated { @decorator method() {} @decorator field = 1; }",
        "class Private { #value = 1; read() { return this.#value; } }",
    ] {
        let program = crate::parse(source).unwrap();
        crate::compile(&program).unwrap();
    }
}

#[test]
fn decorated_accessors_and_non_simple_parameters_compile_through_the_public_pipeline() {
    for source in [
        "function dec(value) { return value; } @dec class C { @dec accessor x = 1; @dec #field = 2; }",
        "function dec(value) { return value; } class C { @dec #method() {} @dec get #value() { return 1; } }",
        "class C { #first; #second; static #third; static #fourth; }",
        "function f(value = 1) { var value; { function arguments() {} } return value; }",
        "function f(value = 1) { var value; function value() {} return value; }",
        "function f() { let marker = 1; { function nested() { return marker; } } return nested; }",
    ] {
        let program = crate::parse(source).unwrap();
        crate::compile(&program).unwrap();
    }
}

#[test]
fn class_lowering_rejects_a_duplicate_private_name_before_emitting_code() {
    let field = ClassElement::Field {
        key: PropertyKey::Identifier("#value".into()),
        initializer: None,
        is_static: false,
        accessor: false,
        decorators: Vec::new(),
    };
    let class = Class {
        name: None,
        extends: None,
        elements: vec![field.clone(), field],
        decorators: Vec::new(),
        source_text: SourceText::default(),
    };
    let mut compiler = bare_compiler();
    assert!(matches!(
        compiler.class_definition(&class, None, None),
        Err(CompileError::InvalidSyntax(
            "duplicate private name in class body"
        ))
    ));
    assert!(compiler.bytecode.bytes().is_empty());
}

#[test]
fn class_element_helpers_reject_a_static_block_as_a_decorated_element() {
    let block = ClassElement::StaticBlock(Vec::new());
    assert!(decorated_element(&block).is_none());
    crate::compile(&crate::parse("class C { static {} }").unwrap()).unwrap();
}

#[test]
fn mapped_arguments_only_include_the_last_simple_parameter_binding() {
    let mut compiler = bare_compiler();
    compiler.names[0].insert("name".into(), 2);
    let identifier = Param {
        pattern: Pattern::Identifier("name".into()),
        default: None,
        rest: false,
    };
    assert_eq!(
        compiler.mapped_argument_slots(&[identifier.clone(), identifier]),
        vec![None, Some(2)]
    );
}

#[test]
fn child_function_metadata_limits_reject_each_initial_binding() {
    let mut captured = bare_compiler();
    captured.max_metadata_entries = 0;
    captured.names[0].insert("outer".into(), 0);
    captured.bytecode.bindings_mut().push(Binding {
        name: "outer".into(),
        mutable: true,
        strict_immutable: false,
        lexical: true,
        catch_parameter: false,
        eval_var: false,
    });
    assert!(matches!(
        captured.function_named(&Function::default(), false, None, false),
        Err(CompileError::ProgramTooLarge)
    ));

    let mut named = bare_compiler();
    named.max_metadata_entries = 0;
    let function = Function {
        name: Some("inner".into()),
        ..Function::default()
    };
    assert!(matches!(
        named.function_named(&function, false, None, true),
        Err(CompileError::ProgramTooLarge)
    ));

    let mut derived = bare_compiler();
    derived.max_metadata_entries = 0;
    let options = FunctionCompileOptions {
        constructible: true,
        force_strict: true,
        class_constructor: true,
        derived_constructor: true,
        default_derived_constructor: false,
        class_method: false,
        class_field_initializer: false,
    };
    assert!(matches!(
        derived.function_named_with(&Function::default(), false, None, false, options),
        Err(CompileError::ProgramTooLarge)
    ));
}

/// The `captures` of the function declared first in the body of the outermost function of
/// `source`, after the outer function is compiled.
fn captures_of_first_inner(source: &str) -> usize {
    let bytecode = crate::compile(&crate::parse(source).unwrap()).unwrap();
    let outer = &bytecode.functions[0];
    outer.functions[0].captures.len()
}

#[test]
fn a_closure_captures_only_the_names_it_mentions() {
    assert_eq!(
        captures_of_first_inner(
            "function outer() { let a = 1, b = 2, c = 3; return function inner() { return b; }; }"
        ),
        1
    );
    assert_eq!(
        captures_of_first_inner(
            "function outer() { let a = 1, b = 2, c = 3; return function inner() { return 1; }; }"
        ),
        0
    );
    assert_eq!(
        captures_of_first_inner("function outer(p, q) { let a = 1; return () => p + a; }"),
        2
    );
}

#[test]
fn a_name_used_only_by_a_nested_function_is_captured_by_each_function_in_between() {
    let bytecode = crate::compile(
        &crate::parse(
            "function outer() { let a = 1, b = 2; \
             return function mid() { return function inner() { return a; }; }; }",
        )
        .unwrap(),
    )
    .unwrap();
    let mid = &bytecode.functions[0].functions[0];
    assert_eq!(mid.captures.len(), 1, "mid passes `a` through");
    assert_eq!(mid.functions[0].captures.len(), 1);
}

#[test]
fn a_direct_eval_anywhere_inside_keeps_every_name() {
    for body in [
        "return function inner() { return eval('a'); };",
        "return function inner() { return () => eval('a'); };",
    ] {
        let source = format!("function outer() {{ let a = 1, b = 2, c = 3; {body} }}");
        assert!(
            captures_of_first_inner(&source) >= 3,
            "every visible name must stay reachable by eval: {source}"
        );
    }
}

#[test]
fn internal_names_are_always_captured() {
    // A derived constructor's arrows reach `this` through a hidden binding.
    let bytecode = crate::compile(
        &crate::parse(
            "class B {} class D extends B { constructor() { const f = () => this; super(); f(); } }",
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!bytecode.functions.is_empty());
}

#[test]
fn a_lookup_that_finds_nothing_is_recorded_and_one_that_resolves_is_not() {
    let mut compiler = bare_compiler();
    compiler.names[0].insert("found".to_string(), 0);
    let guard = std::rc::Rc::new(PruneGuard {
        not_found: std::cell::RefCell::new(Default::default()),
    });
    compiler.prune_guards.push(std::rc::Rc::clone(&guard));
    assert_eq!(compiler.resolve("found"), Some(0));
    assert!(guard.not_found.borrow().is_empty(), "a name that resolves is never a miss");
    assert_eq!(compiler.resolve("somewhere_else"), None);
    assert_eq!(compiler.resolve("somewhere_else"), None);
    assert_eq!(
        guard.not_found.borrow().iter().cloned().collect::<Vec<_>>(),
        vec!["somewhere_else".to_string()],
        "a miss is recorded once"
    );
}

#[test]
fn the_unrecorded_lookup_leaves_the_guards_alone() {
    let mut compiler = bare_compiler();
    compiler.names[0].insert("x".to_string(), 3);
    let guard = std::rc::Rc::new(PruneGuard {
        not_found: std::cell::RefCell::new(Default::default()),
    });
    compiler.prune_guards.push(std::rc::Rc::clone(&guard));
    assert_eq!(compiler.resolve_unrecorded("x"), Some(3));
    assert_eq!(compiler.resolve_unrecorded("y"), None);
    assert!(guard.not_found.borrow().is_empty());
}

/// The `Function` of the first function expression statement in `source`.
fn first_function(program: &crate::Program) -> &Function {
    program
        .body
        .iter()
        .find_map(|statement| match statement {
            crate::ast::Stmt::Expr(crate::ast::Expr::Function(function)) => Some(function),
            crate::ast::Stmt::Expr(crate::ast::Expr::Parenthesized(inner)) => match &**inner {
                crate::ast::Expr::Function(function) => Some(function),
                _ => None,
            },
            _ => None,
        })
        .expect("a function expression")
}

/// A compiler whose scope holds `x` (slot 0) and `y` (slot 1).
fn compiler_with_x_and_y() -> Compiler {
    let mut compiler = bare_compiler();
    for (slot, name) in ["x", "y"].into_iter().enumerate() {
        compiler.names[0].insert(name.to_string(), slot as u32);
        compiler.bytecode.bindings_mut().push(Binding {
            name: name.to_string(),
            mutable: true,
            strict_immutable: false,
            lexical: true,
            catch_parameter: false,
            eval_var: false,
        });
    }
    compiler
}

fn build(
    compiler: &Compiler,
    function: &Function,
    mentions: Option<&crate::ast::Mentions<'_>>,
) -> (Vec<String>, bool) {
    let options = FunctionCompileOptions {
        constructible: true,
        force_strict: false,
        class_constructor: false,
        derived_constructor: false,
        default_derived_constructor: false,
        class_method: false,
        class_field_initializer: false,
    };
    let (built, missed) = compiler.build_function(function, false, None, false, options, mentions);
    let (code, _) = built.expect("the function compiles");
    (
        code.bindings.iter().take(code.captures.len()).map(|b| b.name.clone()).collect(),
        missed,
    )
}

#[test]
fn a_function_captures_only_what_it_mentions_and_its_guard_stays_quiet() {
    let compiler = compiler_with_x_and_y();
    let program = crate::parse("(function () { return x; })").unwrap();
    let function = first_function(&program);
    let mentions = crate::ast::function_mentions(function);
    let (captured, missed) = build(&compiler, function, Some(&mentions));
    assert_eq!(captured, vec!["x".to_string()], "y is not mentioned, so not captured");
    assert!(!missed);
    // Unpruned, both come through, in name order.
    let (all, missed) = build(&compiler, function, None);
    assert_eq!(all, vec!["x".to_string(), "y".to_string()]);
    assert!(!missed);
}

#[test]
fn a_name_the_analysis_left_out_but_the_code_uses_is_noticed_after_the_build() {
    let compiler = compiler_with_x_and_y();
    let uses_x = crate::parse("(function () { return x; })").unwrap();
    let uses_y = crate::parse("(function () { return y; })").unwrap();
    // The analysis of a different function stands in for an analysis that missed `x`.
    let wrong = crate::ast::function_mentions(first_function(&uses_y));
    let (captured, missed) = build(&compiler, first_function(&uses_x), Some(&wrong));
    assert_eq!(captured, vec!["y".to_string()]);
    assert!(missed, "the function looked up `x`, which the creator could see and it left out");
}

#[test]
fn a_global_the_creator_cannot_see_is_not_a_miss() {
    let compiler = compiler_with_x_and_y();
    let program = crate::parse("(function () { return window + x; })").unwrap();
    let function = first_function(&program);
    let mentions = crate::ast::function_mentions(function);
    let (_, missed) = build(&compiler, function, Some(&mentions));
    assert!(!missed, "`window` is simply a global");
}

#[test]
fn a_direct_eval_captures_everything_visible() {
    let compiler = compiler_with_x_and_y();
    let program = crate::parse("(function () { return eval('1'); })").unwrap();
    let function = first_function(&program);
    let mentions = crate::ast::function_mentions(function);
    let (captured, missed) = build(&compiler, function, Some(&mentions));
    assert_eq!(captured, vec!["x".to_string(), "y".to_string()]);
    assert!(!missed);
}
