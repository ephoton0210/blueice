// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn bare_compiler() -> Compiler {
    let limits = CompileLimits::default();
    Compiler {
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
        local_scope: 0,
        with_depth: 0,
        with_scope_depths: Vec::new(),
        annex_b_parameter_names: BTreeSet::new(),
        tail_call_blockers: 0,
        tail_call_pending: false,
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
    assert!(element_decorators(&block).is_empty());
    let decoration = ElementDecoration {
        decorators: String::new(),
        result: String::new(),
        original: String::new(),
        setter: String::new(),
        kind: deco::METHOD,
        is_static: false,
    };
    assert!(matches!(
        bare_compiler().decorate_class_element(&block, &decoration, None, &HashMap::new(), "",),
        Err(CompileError::InvalidSyntax(
            "a static block cannot have decorators"
        ))
    ));
}

#[test]
fn mapped_arguments_only_include_the_last_simple_parameter_binding() {
    let mut compiler = bare_compiler();
    compiler.names[0].insert("name".into(), 2);
    let parameter = Param {
        pattern: Pattern::Array(Vec::new()),
        default: None,
        rest: false,
    };
    let identifier = Param {
        pattern: Pattern::Identifier("name".into()),
        default: None,
        rest: false,
    };
    assert_eq!(compiler.mapped_argument_slots(&[parameter]), vec![None]);
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
    captured.bytecode.bindings.push(Binding {
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

#[test]
fn child_function_budget_accounting_rejects_unrepresentable_cost() {
    let mut compiler = bare_compiler();
    compiler.max_bytecode_bytes = 5;
    assert!(matches!(
        compiler.finish_child_function(Bytecode::empty(), 5, 0, 6),
        Err(CompileError::ProgramTooLarge)
    ));
    assert_eq!(compiler.max_bytecode_bytes, 5);
    assert!(compiler.bytecode.functions.is_empty());
    assert!(matches!(
        compiler.finish_child_function(Bytecode::empty(), 5, 6, 0),
        Err(CompileError::ProgramTooLarge)
    ));
    assert!(matches!(
        compiler.finish_child_function(Bytecode::empty(), u32::MAX, 0, 1),
        Err(CompileError::ProgramTooLarge)
    ));
}
